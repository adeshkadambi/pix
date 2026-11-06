//! Scoreboard-key watch (SPEC §5.11): about 60 Hz on its own thread, only during a game and only
//! with the summoner timer feature on. Shows the tab panel while the key is held with League in
//! front; hides it on release. Reads key state passively; writes nothing to the input stream.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::AppHandle;

use super::{scoreboard_key, win32};

const POLL: Duration = Duration::from_millis(16);
/// Consecutive reads needed to change state, so a single odd read doesn't flicker the panel.
const DEBOUNCE: u8 = 2;

pub struct TabWatch {
    stop: Arc<AtomicBool>,
}

impl TabWatch {
    pub fn start(app: AppHandle, vk: i32) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("pix-tab-watch".into())
            .spawn(move || {
                let mut shown = false;
                let mut streak = 0u8;
                while !flag.load(Ordering::Relaxed) {
                    let want = win32::league_in_front() && win32::key_down(vk);
                    if want == shown {
                        streak = 0;
                    } else {
                        streak += 1;
                        if streak >= DEBOUNCE {
                            shown = want;
                            streak = 0;
                            scoreboard_key(&app, shown);
                        }
                    }
                    std::thread::sleep(POLL);
                }
                if shown {
                    scoreboard_key(&app, false);
                }
            })
            .expect("tab watch thread starts");
        tracing::info!("tab watch started");
        Self { stop }
    }
}

impl Drop for TabWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        tracing::info!("tab watch stopped");
    }
}
