use std::sync::Arc;

use axum::{body::Bytes, extract::ws::{Message, WebSocket}};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use webrtc::{
    api::APIBuilder,
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
    peer_connection::configuration::RTCConfiguration,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

use crate::host_control::ControlWrapper;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, BoxError>;

pub struct WebRtcCore {
    api: webrtc::api::API,
    control_wrapper: Arc<dyn ControlWrapper>
}

impl WebRtcCore {
    pub async fn new(
        control_wrapper: Arc<dyn ControlWrapper>
    ) -> Self {
        let api = APIBuilder::new().build();
        Self { api, control_wrapper }
    }

    pub async fn handle_signaling(
        &self,
        socket: WebSocket,
    ) -> Result<()> {
        let (mut sender, mut receiver) = socket.split();
        let (tx, mut rx): (UnboundedSender<Message>, _) = unbounded_channel();

        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let _ = sender.send(message).await;
            }
        });

        let config = RTCConfiguration {
            ice_servers: vec![],
            ..Default::default()
        };

        let pc = Arc::new(self.api.new_peer_connection(config).await?);
        let tx = Arc::new(tx);
        let cursor_control = self.control_wrapper.get_cursor_control();
        let (screen_width, screen_height) = self.control_wrapper.get_screen_info().get_screen_dimensions();

        pc.on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
            let cursor_control = cursor_control.clone();

            Box::pin(async move {
                let channel2 = channel.clone();
                channel.on_message(Box::new(move |message: DataChannelMessage| {
                    let cursor_control = cursor_control.clone();
                    let channel2 = channel2.clone();
                    Box::pin(async move {
                        if !message.is_string {
                            let data = message.data;
                            let id = u16::from_le_bytes([data[0], data[1]]);
                            if id == 0 {
                                let mut data = data.to_vec();
                                data[0] = 8;
                                channel2.send(&Bytes::from(data)).await.unwrap();
                            } else if id == 2 {
                                let x_ratio = f32::from_le_bytes([data[2], data[3], data[4], data[5]]);
                                let y_ratio = f32::from_le_bytes([data[6], data[7], data[8], data[9]]);
                                let pressure = f32::from_le_bytes([data[10], data[11], data[12], data[13]]);
                                
                                let x = x_ratio.clamp(0.0, 1.0) as f64 * screen_width as f64;
                                let y = y_ratio.clamp(0.0, 1.0) as f64 * screen_height as f64;
                                cursor_control.move_cursor(x, y).await.unwrap();
                                (if pressure > 0.0 {
                                    cursor_control.press_button(1)
                                } else {
                                    cursor_control.release_button(1)
                                }).await.unwrap();
                            }
                        }
                    })
                }));
            })
        }));

        let tx_candidate = tx.clone();
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let tx_candidate = tx_candidate.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(candidate_json) = candidate.to_json() {
                        let payload = json!({
                            "type": "candidate",
                            "candidate": candidate_json
                        })
                        .to_string();

                        let _ = tx_candidate.send(Message::Text(payload));
                    }
                }
            })
        }));

        let init_message = json!({
            "type": "screen-dimensions",
            "width": screen_width,
            "height": screen_height
        }).to_string();
        let _ = tx.send(Message::Text(init_message));

        while let Some(Ok(message)) = receiver.next().await {
            match message {
                Message::Text(payload) => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&payload) {
                        match parsed.get("type").and_then(|value| value.as_str()) {
                            Some("offer") => {
                                if let Some(sdp) = parsed.get("sdp").and_then(|value| value.as_str())
                                {
                                    let remote_desc = RTCSessionDescription::offer(sdp.to_string())?;
                                    pc.set_remote_description(remote_desc).await?;
                                    let answer = pc.create_answer(None).await?;
                                    pc.set_local_description(answer.clone()).await?;
                                    let response = json!({
                                        "type": "answer",
                                        "sdp": answer.sdp
                                    })
                                    .to_string();
                                    let _ = tx.send(Message::Text(response));
                                }
                            }
                            Some("candidate") => {
                                if let Some(candidate_value) = parsed.get("candidate") {
                                    if let Ok(candidate_init) =
                                        serde_json::from_value::<RTCIceCandidateInit>(
                                            candidate_value.clone(),
                                        )
                                    {
                                        let _ = pc.add_ice_candidate(candidate_init).await;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }

        let _ = pc.close().await;
        Ok(())
    }
}