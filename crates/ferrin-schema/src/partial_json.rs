//! Repair and parse truncated JSON.
//!
//! Streaming models emit JSON text incrementally. [`repair`] closes open
//! strings, arrays and objects, completes partial literals (`tru` → `true`)
//! and drops trailing separators so that any prefix of a valid document
//! becomes parseable. [`parse_partial`] tries a direct parse first and falls
//! back to repair.

use std::borrow::Cow;

use serde_json::Value;

/// Outcome of [`parse_partial`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialParse {
    /// The parsed value, if parsing (possibly after repair) succeeded.
    pub value: Option<Value>,
    /// How the value was obtained.
    pub state: PartialParseState,
}

/// How a partial parse succeeded or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PartialParseState {
    /// The input parsed as-is.
    SuccessfulParse,
    /// The input parsed after repair.
    RepairedParse,
    /// The input could not be parsed even after repair.
    FailedParse,
}

/// Parses `text`, repairing it first if a direct parse fails.
#[must_use]
pub fn parse_partial(text: &str) -> PartialParse {
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return PartialParse {
            value: Some(value),
            state: PartialParseState::SuccessfulParse,
        };
    }
    let repaired = repair(text);
    match serde_json::from_str::<Value>(&repaired) {
        Ok(value) => PartialParse {
            value: Some(value),
            state: PartialParseState::RepairedParse,
        },
        Err(_) => PartialParse {
            value: None,
            state: PartialParseState::FailedParse,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Root,
    Finish,
    InsideString,
    InsideStringEscape,
    InsideStringUnicodeEscape,
    InsideLiteral,
    InsideNumber,
    InsideObjectStart,
    InsideObjectKey,
    InsideObjectAfterKey,
    InsideObjectBeforeValue,
    InsideObjectAfterValue,
    InsideObjectAfterComma,
    InsideArrayStart,
    InsideArrayAfterValue,
    InsideArrayAfterComma,
}

struct Repairer<'a> {
    input: &'a str,
    stack: Vec<State>,
    /// Byte index one past the last character that belongs to valid output.
    last_valid_end: usize,
    literal_start: usize,
    unicode_escape_digits: u8,
}

impl Repairer<'_> {
    fn top(&self) -> State {
        // The stack always holds at least `Root`/`Finish`.
        self.stack.last().copied().unwrap_or(State::Finish)
    }

    fn replace_top(&mut self, state: State) {
        self.stack.pop();
        self.stack.push(state);
    }

    fn process_value_start(&mut self, ch: char, start: usize, end: usize, swap: State) {
        match ch {
            '"' => {
                self.last_valid_end = end;
                self.replace_top(swap);
                self.stack.push(State::InsideString);
            }
            'f' | 't' | 'n' => {
                self.last_valid_end = end;
                self.literal_start = start;
                self.replace_top(swap);
                self.stack.push(State::InsideLiteral);
            }
            '-' => {
                self.replace_top(swap);
                self.stack.push(State::InsideNumber);
            }
            '0'..='9' => {
                self.last_valid_end = end;
                self.replace_top(swap);
                self.stack.push(State::InsideNumber);
            }
            '{' => {
                self.last_valid_end = end;
                self.replace_top(swap);
                self.stack.push(State::InsideObjectStart);
            }
            '[' => {
                self.last_valid_end = end;
                self.replace_top(swap);
                self.stack.push(State::InsideArrayStart);
            }
            _ => {}
        }
    }

    fn process_after_object_value(&mut self, ch: char, end: usize) {
        match ch {
            ',' => self.replace_top(State::InsideObjectAfterComma),
            '}' => {
                self.last_valid_end = end;
                self.stack.pop();
            }
            _ => {}
        }
    }

    fn process_after_array_value(&mut self, ch: char, end: usize) {
        match ch {
            ',' => self.replace_top(State::InsideArrayAfterComma),
            ']' => {
                self.last_valid_end = end;
                self.stack.pop();
            }
            _ => {}
        }
    }

    fn step(&mut self, ch: char, start: usize, end: usize) {
        match self.top() {
            State::Root => self.process_value_start(ch, start, end, State::Finish),
            State::Finish => {}
            State::InsideObjectStart => match ch {
                '"' => self.replace_top(State::InsideObjectKey),
                '}' => {
                    self.last_valid_end = end;
                    self.stack.pop();
                }
                _ => {}
            },
            State::InsideObjectAfterComma => {
                if ch == '"' {
                    self.replace_top(State::InsideObjectKey);
                }
            }
            State::InsideObjectKey => {
                if ch == '"' {
                    self.replace_top(State::InsideObjectAfterKey);
                }
            }
            State::InsideObjectAfterKey => {
                if ch == ':' {
                    self.replace_top(State::InsideObjectBeforeValue);
                }
            }
            State::InsideObjectBeforeValue => {
                self.process_value_start(ch, start, end, State::InsideObjectAfterValue);
            }
            State::InsideObjectAfterValue => self.process_after_object_value(ch, end),
            State::InsideString => match ch {
                '"' => {
                    self.stack.pop();
                    self.last_valid_end = end;
                }
                '\\' => self.stack.push(State::InsideStringEscape),
                _ => self.last_valid_end = end,
            },
            State::InsideArrayStart => {
                if ch == ']' {
                    self.last_valid_end = end;
                    self.stack.pop();
                } else {
                    // Whitespace before the first element is valid output; a
                    // lone `-` is not (it would leave `[-]`).
                    if ch != '-' {
                        self.last_valid_end = end;
                    }
                    self.process_value_start(ch, start, end, State::InsideArrayAfterValue);
                }
            }
            State::InsideArrayAfterValue => match ch {
                ',' => self.replace_top(State::InsideArrayAfterComma),
                ']' => {
                    self.last_valid_end = end;
                    self.stack.pop();
                }
                _ => self.last_valid_end = end,
            },
            State::InsideArrayAfterComma => {
                self.process_value_start(ch, start, end, State::InsideArrayAfterValue);
            }
            State::InsideStringEscape => {
                self.stack.pop();
                if ch == 'u' {
                    self.unicode_escape_digits = 0;
                    self.stack.push(State::InsideStringUnicodeEscape);
                } else {
                    self.last_valid_end = end;
                }
            }
            State::InsideStringUnicodeEscape => {
                if ch.is_ascii_hexdigit() {
                    self.unicode_escape_digits += 1;
                    if self.unicode_escape_digits == 4 {
                        self.stack.pop();
                        self.last_valid_end = end;
                    }
                }
            }
            State::InsideNumber => match ch {
                '0'..='9' => self.last_valid_end = end,
                'e' | 'E' | '-' | '.' => {}
                ',' => {
                    self.stack.pop();
                    if self.top() == State::InsideArrayAfterValue {
                        self.process_after_array_value(ch, end);
                    }
                    if self.top() == State::InsideObjectAfterValue {
                        self.process_after_object_value(ch, end);
                    }
                }
                '}' => {
                    self.stack.pop();
                    if self.top() == State::InsideObjectAfterValue {
                        self.process_after_object_value(ch, end);
                    }
                }
                ']' => {
                    self.stack.pop();
                    if self.top() == State::InsideArrayAfterValue {
                        self.process_after_array_value(ch, end);
                    }
                }
                _ => {
                    self.stack.pop();
                }
            },
            State::InsideLiteral => {
                let partial = &self.input[self.literal_start..end];
                if !"false".starts_with(partial)
                    && !"true".starts_with(partial)
                    && !"null".starts_with(partial)
                {
                    self.stack.pop();
                    if self.top() == State::InsideObjectAfterValue {
                        self.process_after_object_value(ch, end);
                    } else if self.top() == State::InsideArrayAfterValue {
                        self.process_after_array_value(ch, end);
                    }
                } else {
                    self.last_valid_end = end;
                }
            }
        }
    }

    fn finish(self) -> String {
        let mut result = String::with_capacity(self.last_valid_end + self.stack.len());
        result.push_str(&self.input[..self.last_valid_end]);
        for state in self.stack.iter().rev() {
            match state {
                State::InsideString => result.push('"'),
                State::InsideObjectKey
                | State::InsideObjectAfterKey
                | State::InsideObjectAfterComma
                | State::InsideObjectStart
                | State::InsideObjectBeforeValue
                | State::InsideObjectAfterValue => result.push('}'),
                State::InsideArrayStart
                | State::InsideArrayAfterComma
                | State::InsideArrayAfterValue => result.push(']'),
                State::InsideLiteral => {
                    let partial = &self.input[self.literal_start..];
                    for literal in ["true", "false", "null"] {
                        if let Some(rest) = literal.strip_prefix(partial) {
                            result.push_str(rest);
                            break;
                        }
                    }
                }
                State::Root
                | State::Finish
                | State::InsideStringEscape
                | State::InsideStringUnicodeEscape
                | State::InsideNumber => {}
            }
        }
        result
    }
}

/// Repairs truncated JSON so that it parses.
///
/// Returns the input unchanged (borrowed) when no repair was needed.
#[must_use]
pub fn repair(input: &str) -> Cow<'_, str> {
    let mut repairer = Repairer {
        input,
        stack: vec![State::Root],
        last_valid_end: 0,
        literal_start: 0,
        unicode_escape_digits: 0,
    };
    for (start, ch) in input.char_indices() {
        let end = start + ch.len_utf8();
        repairer.step(ch, start, end);
    }
    let result = repairer.finish();
    if result == input {
        Cow::Borrowed(input)
    } else {
        Cow::Owned(result)
    }
}
