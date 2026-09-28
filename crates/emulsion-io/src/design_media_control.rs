//! A bounded semantic command channel for the trusted local player page.
use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::Mutex};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaybackCommand {
    Play,
    Pause,
    Seek { position_ms: u32 },
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PlaybackState {
    pub ready: bool,
    pub paused: bool,
    pub position_ms: u32,
    pub duration_ms: Option<u32>,
    pub error_code: Option<u16>,
    pub pending_commands: usize,
}
#[derive(Default)]
pub(super) struct Controls {
    queue: VecDeque<PlaybackCommand>,
    state: PlaybackState,
}
pub(super) type SharedControls = Arc<Mutex<Controls>>;
impl PlayerServer {
    pub fn playback_state(&self) -> PlaybackState {
        let controls = self.controls.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = controls.state.clone();
        state.pending_commands = controls.queue.len();
        state
    }
    pub fn command(&self, command: PlaybackCommand) -> Result<(), String> {
        let mut controls = self
            .controls
            .lock()
            .map_err(|_| "Media control channel unavailable")?;
        if controls.queue.len() >= 32 {
            return Err("Media player command queue is full. Wait for playback to respond.".into());
        }
        if let PlaybackCommand::Seek { position_ms } = command {
            if position_ms > 86_400_000
                || controls
                    .state
                    .duration_ms
                    .is_some_and(|duration| position_ms > duration)
            {
                return Err("Seek position is outside the media duration.".into());
            }
            if !controls.state.ready {
                return Err("Wait for the media player to become ready before seeking.".into());
            }
        }
        controls.queue.push_back(command);
        Ok(())
    }
}
pub(super) fn control_response(
    requested: &str,
    path: &str,
    head: bool,
    controls: &SharedControls,
) -> Option<(String, String)> {
    let query = requested.strip_prefix(&format!("{path}/control?"))?;
    if head {
        return Some(("405 Method Not Allowed".into(), "{}".into()));
    }
    let parsed = (|| -> Option<PlaybackState> {
        let mut fields = std::collections::BTreeMap::new();
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=')?;
            if fields.insert(key, value).is_some() {
                return None;
            }
        }
        if fields.len() != 5 {
            return None;
        }
        let ready = match *fields.get("ready")? {
            "1" => true,
            "0" => false,
            _ => return None,
        };
        let paused = match *fields.get("paused")? {
            "1" => true,
            "0" => false,
            _ => return None,
        };
        let position_ms = fields.get("position_ms")?.parse::<u32>().ok()?;
        let duration = fields.get("duration_ms")?.parse::<u32>().ok()?;
        let error = fields.get("error")?.parse::<u16>().ok()?;
        if position_ms > 86_400_000 || duration > 86_400_000 || error > 1000 {
            return None;
        }
        Some(PlaybackState {
            ready,
            paused,
            position_ms,
            duration_ms: (duration > 0).then_some(duration),
            error_code: (error > 0).then_some(error),
            pending_commands: 0,
        })
    })();
    let Some(state) = parsed else {
        return Some(("400 Bad Request".into(), "{}".into()));
    };
    let mut controls = controls.lock().unwrap_or_else(|e| e.into_inner());
    controls.state = state;
    let commands: Vec<_> = if controls.state.ready {
        controls.queue.drain(..).collect()
    } else {
        Vec::new()
    };
    Some(("200 OK".into(), serde_json::to_string(&commands).unwrap()))
}
/// Both adapters implement a fixed five-method interface. No arbitrary script or URL crosses the channel.
pub(super) fn script(path: &str, youtube: bool) -> String {
    let adapter = if youtube {
        "let p,apiReady=false,apiError=0;window.onYouTubeIframeAPIReady=()=>{p=new YT.Player('youtube',{events:{onReady:()=>{apiReady=true;},onError:e=>{apiError=e.data;}}});};const adapter={ready:()=>apiReady,paused:()=>!apiReady||p.getPlayerState()!==1,time:()=>apiReady?p.getCurrentTime():0,duration:()=>apiReady?p.getDuration():0,error:()=>apiError,play:()=>p.playVideo(),pause:()=>p.pauseVideo(),seek:s=>p.seekTo(s,true)};const api=document.createElement('script');api.src='https://www.youtube.com/iframe_api';document.head.appendChild(api);"
    } else {
        "const adapter={ready:()=>ready,paused:()=>m.paused,time:()=>m.currentTime,duration:()=>m.duration,error:()=>m.error?m.error.code:0,play:()=>{completed=false;m.play().catch(()=>{});},pause:()=>m.pause(),seek:s=>{completed=false;m.currentTime=Math.max(start,Math.min(s,end===null?m.duration:end));}};"
    };
    format!(
        r#"<script>{adapter}let controlBusy=false;const boundedMs=v=>Number.isFinite(v)?Math.round(Math.max(0,Math.min(v*1000,86400000))):0;setInterval(async()=>{{if(controlBusy)return;controlBusy=true;try{{const response=await fetch('{path}/control?ready='+(adapter.ready()?1:0)+'&paused='+(adapter.paused()?1:0)+'&position_ms='+boundedMs(adapter.time())+'&duration_ms='+boundedMs(adapter.duration())+'&error='+adapter.error());if(response.ok)for(const c of await response.json()){{if(c.action==='play')adapter.play();else if(c.action==='pause')adapter.pause();else if(c.action==='seek')adapter.seek(c.position_ms/1000);}}}}catch(e){{}}finally{{controlBusy=false;}}}},100);</script>"#
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn design_media_commands_are_validated_bounded_and_observed() {
        let controls: SharedControls = Default::default();
        let ready = "/p/control?ready=1&paused=1&position_ms=100&duration_ms=1000&error=0";
        assert_eq!(
            control_response(ready, "/p", false, &controls).unwrap().0,
            "200 OK"
        );
        assert_eq!(controls.lock().unwrap().state.position_ms, 100);
        assert_eq!(
            control_response(
                "/p/control?ready=1&paused=1&position_ms=100&duration_ms=1000&error=0&extra=1",
                "/p",
                false,
                &controls
            )
            .unwrap()
            .0,
            "400 Bad Request"
        );
        controls
            .lock()
            .unwrap()
            .queue
            .push_back(PlaybackCommand::Seek { position_ms: 500 });
        let response = control_response(ready, "/p", false, &controls).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response.1).unwrap()[0]["position_ms"],
            500
        );
        assert!(controls.lock().unwrap().queue.is_empty());
        assert_eq!(
            control_response(ready, "/p", true, &controls).unwrap().0,
            "405 Method Not Allowed"
        );
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;
    #[test]
    fn design_media_commands_wait_for_ready_and_reject_invalid_seeks() {
        let media = emulsion_core::design::media::LocalMedia::from_bytes(
            "tone.wav".into(),
            b"RIFF\0\0\0\0WAVEdata".to_vec(),
        )
        .unwrap();
        let server = PlayerServer::start_local(&media).unwrap();
        assert!(
            server
                .command(PlaybackCommand::Seek { position_ms: 20 })
                .is_err()
        );
        server.command(PlaybackCommand::Play).unwrap();
        let not_ready = "/p/control?ready=0&paused=1&position_ms=0&duration_ms=0&error=0";
        assert_eq!(
            control_response(not_ready, "/p", false, &server.controls)
                .unwrap()
                .1,
            "[]"
        );
        assert_eq!(server.playback_state().pending_commands, 1);
        let ready = "/p/control?ready=1&paused=1&position_ms=0&duration_ms=1000&error=0";
        assert!(
            control_response(ready, "/p", false, &server.controls)
                .unwrap()
                .1
                .contains("play")
        );
        server
            .command(PlaybackCommand::Seek { position_ms: 500 })
            .unwrap();
        assert!(
            server
                .command(PlaybackCommand::Seek { position_ms: 1001 })
                .is_err()
        );
        for _ in 0..31 {
            server.command(PlaybackCommand::Pause).unwrap();
        }
        assert!(server.command(PlaybackCommand::Play).is_err());
        assert_eq!(server.playback_state().pending_commands, 32);
    }
}
