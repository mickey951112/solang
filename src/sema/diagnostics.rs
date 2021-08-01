use super::ast::{Diagnostic, ErrorType, Level, Namespace, Note};
use crate::parser::pt::Loc;
use serde::Serialize;

impl Level {
    pub fn to_string(&self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warning => "warning",
            Level::Error => "error",
        }
    }
}

impl Diagnostic {
    pub fn debug(pos: Loc, message: String) -> Self {
        Diagnostic {
            level: Level::Debug,
            ty: ErrorType::None,
            pos: Some(pos),
            message,
            notes: Vec::new(),
        }
    }

    pub fn info(pos: Loc, message: String) -> Self {
        Diagnostic {
            level: Level::Info,
            ty: ErrorType::None,
            pos: Some(pos),
            message,
            notes: Vec::new(),
        }
    }

    pub fn parser_error(pos: Loc, message: String) -> Self {
        Diagnostic {
            level: Level::Error,
            ty: ErrorType::ParserError,
            pos: Some(pos),
            message,
            notes: Vec::new(),
        }
    }

    pub fn error(pos: Loc, message: String) -> Self {
        Diagnostic {
            level: Level::Error,
            ty: ErrorType::SyntaxError,
            pos: Some(pos),
            message,
            notes: Vec::new(),
        }
    }

    pub fn decl_error(pos: Loc, message: String) -> Self {
        Diagnostic {
            level: Level::Error,
            ty: ErrorType::DeclarationError,
            pos: Some(pos),
            message,
            notes: Vec::new(),
        }
    }

    pub fn type_error(pos: Loc, message: String) -> Self {
        Diagnostic {
            level: Level::Error,
            ty: ErrorType::TypeError,
            pos: Some(pos),
            message,
            notes: Vec::new(),
        }
    }

    pub fn warning(pos: Loc, message: String) -> Self {
        Diagnostic {
            level: Level::Warning,
            ty: ErrorType::Warning,
            pos: Some(pos),
            message,
            notes: Vec::new(),
        }
    }

    pub fn warning_with_note(pos: Loc, message: String, note_pos: Loc, note: String) -> Self {
        Diagnostic {
            level: Level::Warning,
            ty: ErrorType::Warning,
            pos: Some(pos),
            message,
            notes: vec![Note {
                pos: note_pos,
                message: note,
            }],
        }
    }

    pub fn warning_with_notes(pos: Loc, message: String, notes: Vec<Note>) -> Self {
        Diagnostic {
            level: Level::Warning,
            ty: ErrorType::Warning,
            pos: Some(pos),
            message,
            notes,
        }
    }

    pub fn error_with_note(pos: Loc, message: String, note_pos: Loc, note: String) -> Self {
        Diagnostic {
            level: Level::Error,
            ty: ErrorType::None,
            pos: Some(pos),
            message,
            notes: vec![Note {
                pos: note_pos,
                message: note,
            }],
        }
    }

    pub fn error_with_notes(pos: Loc, message: String, notes: Vec<Note>) -> Self {
        Diagnostic {
            level: Level::Error,
            ty: ErrorType::None,
            pos: Some(pos),
            message,
            notes,
        }
    }

    fn formated_message(&self, ns: &Namespace) -> String {
        let mut s = if let Some(pos) = self.pos {
            let loc = ns.files[pos.0].loc_to_string(&pos);

            format!("{}: {}: {}", loc, self.level.to_string(), self.message)
        } else {
            format!("solang: {}: {}", self.level.to_string(), self.message)
        };

        for note in &self.notes {
            let loc = ns.files[note.pos.0].loc_to_string(&note.pos);

            s.push_str(&format!("\n\t{}: {}: {}", loc, "note", note.message));
        }

        s
    }
}

pub fn print_messages(ns: &Namespace, debug: bool) {
    for msg in &ns.diagnostics {
        if !debug && msg.level == Level::Debug {
            continue;
        }

        eprintln!("{}", msg.formated_message(ns));
    }
}

/// Do we have any errors
pub fn any_errors(diagnotic: &[Diagnostic]) -> bool {
    diagnotic.iter().any(|m| m.level == Level::Error)
}

#[derive(Serialize)]
pub struct LocJson {
    pub file: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Serialize)]
#[allow(non_snake_case)]
pub struct OutputJson {
    pub sourceLocation: Option<LocJson>,
    #[serde(rename = "type")]
    pub ty: String,
    pub component: String,
    pub severity: String,
    pub message: String,
    pub formattedMessage: String,
}

pub fn message_as_json(ns: &Namespace) -> Vec<OutputJson> {
    let mut json = Vec::new();

    for msg in &ns.diagnostics {
        if msg.level == Level::Info || msg.level == Level::Debug {
            continue;
        }

        let location = msg.pos.map(|pos| LocJson {
            file: format!("{}", ns.files[pos.0].path.display()),
            start: pos.1 + 1,
            end: pos.2 + 1,
        });

        json.push(OutputJson {
            sourceLocation: location,
            ty: format!("{:?}", msg.ty),
            component: "general".to_owned(),
            severity: msg.level.to_string().to_owned(),
            message: msg.message.to_owned(),
            formattedMessage: msg.formated_message(ns),
        });
    }

    json
}
