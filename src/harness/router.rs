use crate::harness::{choice::{ChoiceEvent, ChoiceParser}, power::{LineParseResult, PowerEvent, PowerParser}};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    Power(PowerEvent),
    Choice(ChoiceEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLogEvent {
    pub timestamp: Option<String>,
    pub event: LogEvent,
}

#[derive(Debug, Default)]
pub struct LogRouter {
    power: PowerParser,
    choice: ChoiceParser,
}

impl LogRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.power.reset();
        self.choice.reset();
    }

    pub fn parse_line(&mut self, line: &str) -> Option<ParsedLogEvent> {
        let timestamp = extract_timestamp(line);

        if let Some(event) = self.choice.parse_line(line) {
            return Some(ParsedLogEvent {
                timestamp,
                event: LogEvent::Choice(event),
            });
        }

        match self.power.parse_line(line) {
            LineParseResult::Event(event) => Some(ParsedLogEvent {
                timestamp,
                event: LogEvent::Power(event),
            }),
            LineParseResult::IgnoredNonAuthoritative | LineParseResult::IgnoredAuthoritative => None,
        }
    }
}

pub fn extract_timestamp(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    let level = parts.next()?;
    if !matches!(level, "D" | "W" | "E" | "I") {
        return None;
    }
    Some(parts.next()?.to_owned())
}
