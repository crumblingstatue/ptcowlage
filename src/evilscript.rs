use {
    crate::audio_out::SongState,
    anyhow::Context,
    ptcow::{Event, EventPayload, UnitIdx},
};

pub enum EvilCmd {
    RemoveMatchingEvent {
        predicate: Box<dyn FnMut(&Event) -> bool>,
    },
    Help,
}

pub fn parse(cmd: &str) -> anyhow::Result<EvilCmd> {
    let mut tokens = cmd.split_whitespace();
    let cmd = tokens.next().context("Missing command")?;
    macro_rules! rm_payload {
        ($p:path) => {
            match tokens.next() {
                Some(tok) => {
                    let val = tok.parse()?;
                    Ok(EvilCmd::RemoveMatchingEvent {
                        predicate: Box::new(move |ev| ev.payload == $p(val)),
                    })
                }
                None => Ok(EvilCmd::RemoveMatchingEvent {
                    predicate: Box::new(move |ev| matches!(ev.payload, $p(_))),
                }),
            }
        };
    }
    match cmd {
        "rm" => match tokens.next().context("Remove what?")? {
            "velocity" => rm_payload!(EventPayload::Velocity),
            "volume" => rm_payload!(EventPayload::Volume),
            "panvol" => rm_payload!(EventPayload::PanVol),
            "unit" => {
                let idx: u8 = tokens.next().context("Missing unit number")?.parse()?;
                Ok(EvilCmd::RemoveMatchingEvent {
                    predicate: Box::new(move |ev| ev.unit == UnitIdx(idx)),
                })
            }
            etc => anyhow::bail!("I don't know what a '{etc}' is"),
        },
        "help" => Ok(EvilCmd::Help),
        _ => anyhow::bail!("Unknown evil comand '{cmd}'"),
    }
}

const HELP_STRING: &str = "\
rm <PayloadType (payload_value)> - Remove events matching a payload
rm unit <unit_idx> - Remove events that reference unit of index <unit_idx>
help - Show this help (duh)
";

/// Execute EvilScript command. Returns an optional string output.
pub fn exec(cmd: EvilCmd, song: &mut SongState) -> Option<String> {
    match cmd {
        EvilCmd::RemoveMatchingEvent { mut predicate } => {
            song.song.events.eves.retain(|eve| !predicate(eve));
        }
        EvilCmd::Help => return Some(HELP_STRING.into()),
    }
    None
}
