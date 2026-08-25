use std::sync::Arc;

use axum::{body::Bytes, extract::ws::{Message, WebSocket}};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::{Mutex, mpsc::{UnboundedSender, unbounded_channel}};
use webrtc::{
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
    peer_connection::configuration::RTCConfiguration,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

use crate::control::ControlWrapper;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, BoxError>;

pub struct WebRtcCore {
    pub api: webrtc::api::API,
    pub control_wrapper: Arc<dyn ControlWrapper>,
    pub rtc_config: Option<RTCConfiguration>,
    webrtc_only: bool
}

impl WebRtcCore {
    pub async fn new(
        api: webrtc::api::API,
        control_wrapper: Arc<dyn ControlWrapper>,
        webrtc_only: bool
    ) -> Self {
        Self { api, control_wrapper, rtc_config: None, webrtc_only }
    }

    pub async fn handle_signaling(
        &self,
        socket: WebSocket,
    ) -> Result<()> {
        let (sender, mut receiver) = socket.split();
        let (sender_tx, mut sender_rx): (UnboundedSender<Message>, _) = unbounded_channel();
        let sender = Arc::new(Mutex::new(sender));
        let (receiver_tx, mut receiver_rx): (UnboundedSender<Bytes>, _) = unbounded_channel();
        let receiver_tx = Arc::new(receiver_tx);
        

        let tx_sender = sender.clone();
        let jh = tokio::spawn(async move {
            while let Some(message) = sender_rx.recv().await {
                if let Message::Close(_) = message { break; }
                let _ = tx_sender.lock().await.send(message).await;
            }
            sender_rx
        });

        let cursor_control = self.control_wrapper.get_cursor_control();
        let (screen_width, screen_height) = self.control_wrapper.get_screen_info().get_screen_dimensions();
        let tx_sender = sender.clone();
        tokio::spawn(async move {
            while let Some(data) = receiver_rx.recv().await {
                let id = u16::from_le_bytes([data[0], data[1]]);
                if id == 0 {
                    let mut data = data.to_vec();
                    data[0] = 8;
                    tx_sender.lock().await.send(Message::Binary(data)).await.unwrap();
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
        });

        let config = self.rtc_config.as_ref().map(|c| c.clone())
            .unwrap_or(RTCConfiguration {
                ice_servers: vec![],
                ..Default::default()
            });

        let pc = Arc::new(self.api.new_peer_connection(config).await?);
        let tx = Arc::new(sender_tx);

        let tx2 = tx.clone();
        let mut jh = Some(jh);
        let receiver_tx_dc = receiver_tx.clone();
        pc.on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
            let tx = tx2.clone();
            let receiver_tx = receiver_tx_dc.clone();
            let jh = jh.take();
            Box::pin(async move {
                let receiver_tx = receiver_tx.clone();
                channel.on_message(Box::new(move |message: DataChannelMessage| {
                    let receiver_tx = receiver_tx.clone();
                    Box::pin(async move {
                        if !message.is_string {
                            let _ = receiver_tx.send(message.data);
                        }
                    })
                }));
                tx.send(Message::Close(None)).unwrap();
                let mut sender_rx = jh.unwrap().await.unwrap();
                tokio::spawn(async move {
                    while let Some(Message::Binary(message)) = sender_rx.recv().await {
                        let _ = channel.send(&Bytes::from(message)).await;
                    }
                });
            })
        }));

        let tx_candidate = sender.clone();
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

                        let _ = tx_candidate.lock().await.send(Message::Text(payload));
                    }
                }
            })
        }));

        let init_message = json!({
            "type": "screen-dimensions",
            "width": screen_width,
            "height": screen_height
        }).to_string();
        let _ = sender.lock().await.send(Message::Text(init_message));

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
                },
                Message::Binary(data) => {
                    if !self.webrtc_only {
                        let _ = receiver_tx.send(Bytes::from(data));
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