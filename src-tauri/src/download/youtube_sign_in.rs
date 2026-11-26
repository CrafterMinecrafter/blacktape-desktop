use crate::auth::{account, youtube};
use crate::AuthLock;
use serde::Deserialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, EventId, Listener, Manager, WebviewWindow};

const REQUEST: &str = "download-youtube-sign-in-required";
const RESPONSE: &str = "download-youtube-sign-in-response";
const CLOSED: &str = "download-youtube-sign-in-dismissed";
const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);
pub(super) const CANCELLED: &str = "YouTube sign-in was cancelled.";

#[derive(Deserialize)]
struct Decision {
    request_id: uuid::Uuid,
    proceed: bool,
}

struct SignInResponse {
    id: uuid::Uuid,
    sender: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
}

impl SignInResponse {
    fn receive(&self, payload: &str) {
        let Ok(decision) = serde_json::from_str::<Decision>(payload) else {
            return;
        };
        if decision.request_id != self.id {
            return;
        }
        if let Some(sender) = self.sender.lock().ok().and_then(|mut sender| sender.take()) {
            let _ = sender.send(decision.proceed);
        }
    }
}

struct PendingPrompt {
    window: WebviewWindow,
    listener: EventId,
    id: uuid::Uuid,
}

impl Drop for PendingPrompt {
    fn drop(&mut self) {
        self.window.unlisten(self.listener);
        let _ = self.window.emit_to("main", CLOSED, self.id);
    }
}

async fn ask_to_sign_in(app: &AppHandle) -> Result<bool, String> {
    let window = app.get_webview_window("main").ok_or("Could not show the YouTube sign-in prompt.")?;
    let id = uuid::Uuid::new_v4();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let response = SignInResponse { id, sender: Mutex::new(Some(sender)) };
    let listener = window.listen(RESPONSE, move |event| response.receive(event.payload()));
    let pending = PendingPrompt { window, listener, id };
    pending.window.emit_to("main", REQUEST, id).map_err(|_| "Could not show the YouTube sign-in prompt.")?;
    tokio::time::timeout(PROMPT_TIMEOUT, receiver).await
        .map_err(|_| "Timed out waiting for the YouTube sign-in prompt.".to_string())?
        .map_err(|_| "The YouTube sign-in prompt was closed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prompt_accepts_only_the_first_valid_reply_for_the_current_request() {
        for proceed in [false, true] {
            let id = uuid::Uuid::new_v4();
            let (sender, mut receiver) = tokio::sync::oneshot::channel();
            let response = SignInResponse { id, sender: Mutex::new(Some(sender)) };
            response.receive("invalid");
            response.receive(&serde_json::json!({"request_id": uuid::Uuid::new_v4(), "proceed": !proceed}).to_string());
            assert!(matches!(receiver.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Empty)));
            response.receive(&serde_json::json!({"request_id": id, "proceed": proceed}).to_string());
            response.receive(&serde_json::json!({"request_id": id, "proceed": !proceed}).to_string());
            assert_eq!(receiver.await.unwrap(), proceed);
        }
    }
}

pub(super) async fn ensure_session(app: &AppHandle, task_id: &str) -> Result<(), String> {
    if account::youtube_download_session_ready(app).await? {
        return Ok(());
    }

    let message = "Waiting for YouTube sign-in…";
    if let Some(registry) = app.try_state::<super::TaskRegistry>() {
        if let Some((current, total)) = registry.set_task_message(task_id, message).await {
            let _ = app.emit("download-task-progress", super::ProgressEvent {
                task_id: task_id.into(), current, total, track_title: message.into(),
            });
        }
    }
    if !ask_to_sign_in(app).await? {
        return Err(CANCELLED.into());
    }

    youtube::launch_youtube_login(app.clone(), false, app.state::<AuthLock>()).await
        .map_err(|error| {
            if error == "Authentication canceled by user." { CANCELLED.into() }
            else { format!("Could not open YouTube sign-in: {error}") }
        })
}
