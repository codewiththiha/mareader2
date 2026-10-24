//! How many audio settings each listed Mac voice reports: none means mute.

use objc2::msg_send;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSString;

/// Each listed voice's id and its count of audio settings.
#[allow(unsafe_code)]
pub fn sounds() -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let Some(class) = AnyClass::get(c"AVSpeechSynthesisVoice") else {
        return out;
    };
    autoreleasepool(|_| {
        // SAFETY: a class method returning an NSArray of voices, or nil.
        let voices: Option<Retained<AnyObject>> = unsafe { msg_send![class, speechVoices] };
        let Some(voices) = voices else {
            return;
        };
        // SAFETY: NSArray's count, an NSUInteger.
        let count: usize = unsafe { msg_send![&*voices, count] };
        for i in 0..count {
            // SAFETY: i is below count; each item is an AVSpeechSynthesisVoice.
            let voice: Retained<AnyObject> = unsafe { msg_send![&*voices, objectAtIndex: i] };
            // SAFETY: both are object properties of AVSpeechSynthesisVoice.
            let id: Option<Retained<NSString>> = unsafe { msg_send![&*voice, identifier] };
            let settings: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*voice, audioFileSettings] };
            // SAFETY: the settings are an NSDictionary; count, an NSUInteger.
            let n: usize = settings.map_or(0, |s| unsafe { msg_send![&*s, count] });
            if let Some(id) = id {
                out.push((id.to_string(), n));
            }
        }
    });
    out
}
