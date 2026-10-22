//! A mute engine: sentences start, then end next look; for tests and shots.

use crate::{Caps, Event, Fault, Tone, Voice, Voicer};

#[derive(Debug, Default)]
pub struct Silent {
    /// Said, and not yet reported started; started, not yet ended.
    said: Option<u64>,
    started: Option<u64>,
    paused: bool,
    voice: Option<String>,
}

impl Voicer for Silent {
    fn caps(&self) -> Caps {
        Caps {
            rate: true,
            pitch: true,
            volume: true,
            voices: true,
            pause: true,
        }
    }

    /// Two voices in two languages, so pickers have something to pick.
    fn voices(&self) -> Vec<Voice> {
        let voice = |id: &str, name: &str, language: &str| Voice {
            id: id.to_owned(),
            name: name.to_owned(),
            language: language.to_owned(),
        };
        vec![
            voice("quiet-en", "Quiet", "en-US"),
            voice("hush-fr", "Hush", "fr-FR"),
        ]
    }

    fn set_voice(&mut self, id: Option<&str>) {
        self.voice = id.map(str::to_owned);
    }

    fn set_tone(&mut self, _tone: Tone) {}

    fn say(&mut self, id: u64, _text: &str) -> Result<(), Fault> {
        self.said = Some(id);
        self.started = None;
        self.paused = false;
        Ok(())
    }

    fn stop(&mut self) {
        self.said = None;
        self.started = None;
    }

    fn pause(&mut self) -> bool {
        self.paused = true;
        true
    }

    fn resume(&mut self) -> bool {
        self.paused = false;
        true
    }

    fn events(&mut self) -> Vec<Event> {
        if self.paused {
            return Vec::new();
        }
        if let Some(id) = self.started.take() {
            return vec![Event::Finished(id)];
        }
        match self.said.take() {
            Some(id) => {
                self.started = Some(id);
                vec![Event::Started(id)]
            }
            None => Vec::new(),
        }
    }
}
