//! Developer aid for end-to-end checks: `SKRIB_DEBUG_SCRIPT=<file>` feeds
//! synthetic input events through egui's normal input pipeline, one step
//! per frame. Not used in normal operation.
//!
//! ```text
//! wait 5                  # idle frames
//! move 300 200            # pointer position, window points
//! click 300 200
//! drag 100 100 400 300 12 # from, to, number of move steps
//! key A                   # key press+release (egui key name)
//! key Z cmd shift         # with modifiers
//! text Hello world        # typed text
//! copy                    # clipboard shortcut events
//! paste
//! hold alt                # keep modifiers pressed for following steps
//! release
//! wheel 0 3 cmd           # scroll lines (with Ctrl/Cmd: zoom)
//! screenshot /tmp/x.png
//! quit
//! ```

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, RawInput, pos2};
use std::collections::VecDeque;
use std::path::PathBuf;

pub(super) enum Step {
    Wait,
    Events(Vec<Event>, Modifiers),
    Hold(Modifiers),
    Screenshot(PathBuf),
    Quit,
}

#[derive(Default)]
pub(super) struct Script {
    steps: VecDeque<Step>,
    pointer: Pos2,
    /// Modifiers held across steps (`hold alt` … `release`).
    held: Modifiers,
}

fn parse_mods(words: &[&str]) -> Modifiers {
    let mut m = Modifiers::NONE;
    for w in words {
        match *w {
            "cmd" => m |= Modifiers::COMMAND,
            "shift" => m |= Modifiers::SHIFT,
            "alt" => m |= Modifiers::ALT,
            _ => {}
        }
    }
    m
}

impl Script {
    pub fn load(path: &std::path::Path) -> Result<Script, String> {
        let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut s = Script::default();
        for (n, line) in src.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| -> Result<f32, String> {
                words
                    .get(i)
                    .and_then(|w| w.parse().ok())
                    .ok_or(format!("line {}: expected number", n + 1))
            };
            match words[0] {
                "wait" => (0..num(1)? as usize).for_each(|_| s.steps.push_back(Step::Wait)),
                "move" => s.move_to(pos2(num(1)?, num(2)?)),
                "click" => {
                    let p = pos2(num(1)?, num(2)?);
                    s.move_to(p);
                    s.button(p, true);
                    s.button(p, false);
                }
                "drag" => {
                    let (a, b) = (pos2(num(1)?, num(2)?), pos2(num(3)?, num(4)?));
                    let steps = num(5).unwrap_or(10.0).max(1.0) as usize;
                    s.move_to(a);
                    s.button(a, true);
                    for i in 1..=steps {
                        s.move_to(a + (b - a) * (i as f32 / steps as f32));
                    }
                    s.button(b, false);
                }
                "key" => {
                    let key = Key::from_name(words.get(1).copied().unwrap_or(""))
                        .ok_or(format!("line {}: unknown key", n + 1))?;
                    let m = parse_mods(&words[2..]);
                    let ev = |pressed| Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: m,
                    };
                    s.steps.push_back(Step::Events(vec![ev(true), ev(false)], m));
                }
                "text" => {
                    let t = line["text".len()..].trim_start().replace("\\n", "\n");
                    s.steps
                        .push_back(Step::Events(vec![Event::Text(t)], Modifiers::NONE));
                }
                "wheel" => {
                    let m = parse_mods(&words[3..]);
                    let ev = Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::vec2(num(1)?, num(2)?),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Wheel,
                        modifiers: m,
                    };
                    s.steps.push_back(Step::Events(vec![ev], m));
                }
                "hold" => s.steps.push_back(Step::Hold(parse_mods(&words[1..]))),
                "release" => s.steps.push_back(Step::Hold(Modifiers::NONE)),
                "copy" => s
                    .steps
                    .push_back(Step::Events(vec![Event::Copy], Modifiers::COMMAND)),
                "paste" => s.steps.push_back(Step::Events(
                    vec![Event::Paste(String::new())],
                    Modifiers::COMMAND,
                )),
                "screenshot" => s.steps.push_back(Step::Screenshot(PathBuf::from(
                    words.get(1).copied().unwrap_or("shot.png"),
                ))),
                "quit" => s.steps.push_back(Step::Quit),
                other => return Err(format!("line {}: unknown command {other}", n + 1)),
            }
        }
        Ok(s)
    }

    fn move_to(&mut self, p: Pos2) {
        self.pointer = p;
        self.steps
            .push_back(Step::Events(vec![Event::PointerMoved(p)], Modifiers::NONE));
    }

    fn button(&mut self, p: Pos2, pressed: bool) {
        self.steps.push_back(Step::Events(
            vec![Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        ));
    }

    /// Feeds the next step into `raw`. Returns screenshot/quit requests.
    pub fn feed(&mut self, ctx: &egui::Context, raw: &mut RawInput) -> Option<Step> {
        let step = self.steps.pop_front()?;
        ctx.request_repaint();
        match step {
            Step::Hold(m) => {
                self.held = m;
                raw.events.push(Event::ModifiersChanged(m));
                None
            }
            Step::Events(events, mods) => {
                let mods = mods | self.held;
                raw.events.push(Event::ModifiersChanged(mods));
                raw.events.extend(events);
                if mods != self.held {
                    // Release modifiers on the next step.
                    self.steps.push_front(Step::Events(Vec::new(), self.held));
                }
                None
            }
            Step::Wait => None,
            other => Some(other),
        }
    }
}
