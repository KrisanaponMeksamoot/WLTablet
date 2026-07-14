use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use axum::extract::ws::{Message, WebSocket};
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

use crate::dbus_portal::CursorControl;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, BoxError>;

pub struct WebRtcCore {
    api: webrtc::api::API,
    cursor_control: Arc<dyn CursorControl>,
    pairing_allowed: Arc<AtomicBool>,
}

impl WebRtcCore {
    pub async fn new(
        cursor_control: Arc<dyn CursorControl>,
        pairing_allowed: Arc<AtomicBool>,
    ) -> Self {
        let api = APIBuilder::new().build();
        Self { api, cursor_control, pairing_allowed }
    }

    pub async fn handle_signaling(
        &self,
        socket: WebSocket,
        _pairing_allowed: Arc<AtomicBool>,
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
        let cursor_control = self.cursor_control.clone();
        let pairing_flag = self.pairing_allowed.clone();

        pc.on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
            let cursor_control = cursor_control.clone();
            let pairing_flag = pairing_flag.clone();

            Box::pin(async move {
                let open_flag = pairing_flag.clone();
                channel.on_open(Box::new(move || {
                    let open_flag = open_flag.clone();
                    Box::pin(async move {
                        open_flag.store(false, Ordering::SeqCst);
                    })
                }));

                let cursor_control = cursor_control.clone();
                channel.on_message(Box::new(move |message: DataChannelMessage| {
                    let cursor_control = cursor_control.clone();
                    Box::pin(async move {
                        if !message.is_string {
                            let data = message.data;
                            if data.len() == 20 {
                                let x_ratio = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                                let y_ratio = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                                let screen_width = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                                let screen_height = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
                                let _pressure = f32::from_le_bytes([data[16], data[17], data[18], data[19]]);
                                
                                let x = (x_ratio * screen_width as f32).clamp(0.0, screen_width as f32) as u32;
                                let y = (y_ratio * screen_height as f32).clamp(0.0, screen_height as f32) as u32;
                                let _ = cursor_control.move_cursor(x, y).await;
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

        let (screen_width, screen_height) = self.cursor_control.get_screen_dimensions();
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