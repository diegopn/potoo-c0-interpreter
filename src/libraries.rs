mod images;
mod numeric;
mod terminal;

use std::{collections::HashMap, io::BufRead, sync::Arc};

use crate::{
    Diagnostic, Location, RuntimeHost,
    heap::{Heap, Memory, Value},
    syntax::{Expression, ExpressionKind, Literal, Type},
};

pub(crate) fn header(name: &str) -> Option<&'static str> {
    Some(match name {
        "conio" => {
            "
            void print(string s);
            void println(string s);
            void printint(int n);
            void printbool(bool b);
            void printchar(char c);
            void flush();
            bool eof();
            string readline() /*@requires !eof(); @*/ ;
            void printf();
        "
        }
        "string" => {
            "
            int string_length(string s);
            char string_charat(string s, int n);
            string string_join(string a, string b);
            string string_sub(string s, int start, int end);
            bool string_equal(string a, string b);
            int string_compare(string a, string b);
            string string_frombool(bool b);
            string string_fromint(int i);
            string string_fromchar(char c) /*@requires c != '\\0'; @*/ ;
            string string_tolower(string s);
            char[] string_to_chararray(string s);
            string string_from_chararray(char[] A);
            bool string_terminated(char[] A, int n);
            int char_ord(char c);
            char char_chr(int n);
            string format();
        "
        }
        "parse" => {
            "
            bool* parse_bool(string s);
            int* parse_int(string s, int base);
            int num_tokens(string s);
            string[] parse_tokens(string s);
            bool int_tokens(string s, int base);
            int[] parse_ints(string s, int base) /*@requires int_tokens(s, base); @*/ ;
        "
        }
        "file" => {
            "
            struct file;
            typedef struct file* file_t;
            bool file_closed(file_t f);
            file_t file_read(string path);
            void file_close(file_t f);
            bool file_eof(file_t f);
            string file_readline(file_t f) /*@requires !file_eof(f); @*/ ;
        "
        }
        "args" => {
            "
            struct args { int argc; string[] argv; };
            void args_flag(string name, bool* value);
            void args_int(string name, int* value);
            void args_string(string name, string* value);
            typedef struct args* args_t;
            args_t args_parse();
        "
        }
        "img" => {
            "
            struct image;
            typedef struct image* image_t;
            image_t image_create(int width, int height);
            image_t image_clone(image_t image);
            void image_destroy(image_t image);
            image_t image_subimage(image_t image, int x, int y, int w, int h);
            image_t image_load(string path);
            void image_save(image_t image, string path);
            int image_width(image_t image);
            int image_height(image_t image);
            int[] image_data(image_t image);
        "
        }
        "rand" => numeric::RAND,
        "util" => numeric::UTIL,
        "fpt" => numeric::FPT,
        "dub" => numeric::DUB,
        "curses" => terminal::HEADER,
        _ => return None,
    })
}

pub(crate) fn is_formatted(name: &str) -> bool {
    matches!(name, "printf" | "format")
}

#[derive(Clone, Copy)]
enum FormatPart<'a> {
    Text(&'a [u8]),
    Int,
    Char,
    String,
}

fn format_parts(format: &[u8], location: Location) -> Result<Vec<FormatPart<'_>>, Diagnostic> {
    let mut remainder = format;
    let mut parts = Vec::new();
    while let Some(marker) = remainder.iter().position(|&byte| byte == b'%') {
        parts.push(FormatPart::Text(&remainder[..marker]));
        let Some((&code, tail)) = remainder[marker + 1..].split_first() else {
            return Err(location.error("incomplete format specifier"));
        };
        parts.push(match code {
            b'%' => FormatPart::Text(b"%"),
            b'd' => FormatPart::Int,
            b'c' => FormatPart::Char,
            b's' => FormatPart::String,
            _ => {
                return Err(location.error(format!(
                    "unsupported format specifier '%{}'",
                    char::from(code)
                )));
            }
        });
        remainder = tail;
    }
    parts.push(FormatPart::Text(remainder));
    Ok(parts)
}

pub(crate) fn format_types(
    args: &[Expression],
    location: Location,
) -> Result<Vec<Type>, Diagnostic> {
    let mut format = args
        .first()
        .ok_or_else(|| location.error("expected a format string literal"))?;
    while let ExpressionKind::Group(inner) = &format.kind {
        format = inner;
    }
    let ExpressionKind::Literal(Literal::String(text)) = &format.kind else {
        return Err(format.location.error("expected a format string literal"));
    };
    let mut types = vec![Type::String];
    types.extend(
        format_parts(text, format.location)?
            .into_iter()
            .filter_map(|part| match part {
                FormatPart::Text(_) => None,
                FormatPart::Int => Some(Type::Int),
                FormatPart::Char => Some(Type::Char),
                FormatPart::String => Some(Type::String),
            }),
    );
    Ok(types)
}

fn formatted(name: &str, args: &[Value], location: Location) -> Result<Vec<u8>, Diagnostic> {
    let invalid = || invalid_arguments(name, location);
    let Some((Value::String(format), args)) = args.split_first() else {
        return Err(invalid());
    };
    let mut args = args.iter();
    let mut output = Vec::new();
    for part in format_parts(format, location)? {
        if let FormatPart::Text(text) = part {
            output.extend_from_slice(text);
            continue;
        }
        match (part, args.next()) {
            (FormatPart::Int, Some(Value::Int(value))) => {
                output.extend_from_slice(value.to_string().as_bytes());
            }
            (FormatPart::Char, Some(Value::Char(value))) => {
                output.push(ascii_char(*value, location)?);
            }
            (FormatPart::String, Some(Value::String(value))) => output.extend_from_slice(value),
            _ => return Err(invalid()),
        }
    }
    if args.next().is_some() {
        return Err(invalid());
    }
    Ok(output)
}

fn invalid_arguments(name: &str, location: Location) -> Diagnostic {
    location.error(format!("invalid arguments for native function '{name}'"))
}

#[derive(Default)]
pub(crate) struct Resources {
    next_id: usize,
    files: HashMap<usize, Box<dyn BufRead>>,
    arguments: HashMap<Arc<[u8]>, Argument>,
    images: HashMap<usize, images::ImageData>,
    windows: HashMap<u64, Memory>,
}

struct Argument {
    kind: ArgumentKind,
    destination: Memory,
}

#[derive(Clone, Copy)]
enum ArgumentKind {
    Flag,
    Int,
    String,
}

impl Resources {
    pub(crate) fn call(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut impl RuntimeHost,
        heap: &mut Heap,
        location: Location,
    ) -> Result<Value, Diagnostic> {
        let io_error = |error| location.error(format!("{name}: {error}"));
        let value = match (name, args) {
            ("print", [Value::String(text)]) => {
                host.write(text).map_err(io_error)?;
                Value::Void
            }
            ("println", [Value::String(text)]) => {
                host.write(text)
                    .and_then(|()| host.write(b"\n"))
                    .map_err(io_error)?;
                Value::Void
            }
            ("printbool", [Value::Bool(value)]) => {
                host.write(value.to_string().as_bytes()).map_err(io_error)?;
                Value::Void
            }
            ("printint", [Value::Int(value)]) => {
                host.write(value.to_string().as_bytes()).map_err(io_error)?;
                Value::Void
            }
            ("printchar", [Value::Char(value)]) => {
                host.write(&[ascii_char(*value, location)?])
                    .map_err(io_error)?;
                Value::Void
            }
            ("printf", args) => {
                host.write(&formatted(name, args, location)?)
                    .map_err(io_error)?;
                Value::Void
            }
            ("format", args) => Value::string(without_nul(formatted(name, args, location)?)),
            ("flush", []) => {
                host.flush().map_err(io_error)?;
                Value::Void
            }
            ("readline", []) => {
                host.flush().map_err(io_error)?;
                let input = host.read_line().map_err(io_error)?.unwrap_or_default();
                Value::string(host_text(input, location)?)
            }
            ("eof", []) => Value::Bool(host.input_eof().map_err(io_error)?),
            ("file_read", [Value::String(path)]) => {
                let reader = match host.open_file(path) {
                    Ok(reader) => reader,
                    Err(error) => return Err(io_error(error)),
                };
                let (id, value) = self.resource(heap);
                self.files.insert(id, reader);
                value
            }
            ("file_closed", [file]) => {
                Value::Bool(!self.files.contains_key(&resource_id(file, location)?))
            }
            ("file_close", [file]) => {
                let id = resource_id(file, location)?;
                if self.files.remove(&id).is_none() {
                    return Err(location.error("file is closed"));
                }
                Value::Void
            }
            ("file_eof", [file]) => Value::Bool(
                self.file(file, location)?
                    .fill_buf()
                    .map_err(io_error)?
                    .is_empty(),
            ),
            ("file_readline", [file]) => {
                let mut line = Vec::new();
                self.file(file, location)?
                    .read_until(b'\n', &mut line)
                    .map_err(io_error)?;
                trim_line_ending(&mut line);
                Value::string(host_text(line, location)?)
            }
            (
                form @ ("args_flag" | "args_int" | "args_string"),
                [Value::String(name), Value::Pointer(destination)],
            ) => {
                let destination = destination
                    .as_ref()
                    .ok_or_else(|| location.error("argument destination is NULL"))?;
                let kind = match form {
                    "args_flag" => ArgumentKind::Flag,
                    "args_int" => ArgumentKind::Int,
                    _ => ArgumentKind::String,
                };
                self.register_argument(name, destination, kind, location)?
            }
            ("args_parse", []) => {
                self.parse_arguments(host.arguments().map_err(io_error)?, heap, location)?
            }
            ("parse_bool", [Value::String(text)]) => {
                let value = match text.as_ref() {
                    b"true" => Some(Value::Bool(true)),
                    b"false" => Some(Value::Bool(false)),
                    _ => None,
                };
                pointer(heap, value)
            }
            ("parse_int", [Value::String(text), Value::Int(base)]) => {
                pointer(heap, parse_int(text, *base, location)?.map(Value::Int))
            }
            ("num_tokens", [Value::String(text)]) => {
                Value::Int(length(tokens(text).count(), location)?)
            }
            ("parse_tokens", [Value::String(text)]) => {
                Value::Array(Some(heap.block(tokens(text).map(Value::string).collect())))
            }
            ("int_tokens" | "parse_ints", [Value::String(text), Value::Int(base)]) => {
                let base = parse_base(*base, location)?;
                let numbers: Option<Vec<_>> = tokens(text)
                    .map(|token| integer(token, base).map(Value::Int))
                    .collect();
                if name == "int_tokens" {
                    Value::Bool(numbers.is_some())
                } else {
                    let numbers = numbers.ok_or_else(|| {
                        location.error("string contains an invalid integer token")
                    })?;
                    Value::Array(Some(heap.block(numbers)))
                }
            }
            _ if name.starts_with("image_") => self.image(name, args, host, heap, location)?,
            _ if name.starts_with("c_") || name.starts_with("cc_") => {
                self.terminal(name, args, host, heap, location)?
            }
            _ if name.starts_with("string_") || name.starts_with("char_") => {
                strings(name, args, heap, location)?
            }
            _ => numeric::call(name, args, host, heap, location)?,
        };
        Ok(value)
    }

    fn resource(&mut self, heap: &mut Heap) -> (usize, Value) {
        let id = self.next_id;
        self.next_id += 1;
        (id, pointer(heap, Some(Value::Resource(id))))
    }

    fn file(&mut self, file: &Value, location: Location) -> Result<&mut dyn BufRead, Diagnostic> {
        match self.files.get_mut(&resource_id(file, location)?) {
            Some(reader) => Ok(reader.as_mut()),
            None => Err(location.error("file is closed")),
        }
    }

    fn register_argument(
        &mut self,
        name: &Arc<[u8]>,
        destination: &Memory,
        kind: ArgumentKind,
        location: Location,
    ) -> Result<Value, Diagnostic> {
        let option: Arc<[u8]> = [b"-", name.as_ref()].concat().into();
        if self.arguments.contains_key(&option) {
            return Err(location.error("argument name is already registered"));
        }
        self.arguments.insert(
            option,
            Argument {
                kind,
                destination: destination.clone(),
            },
        );
        Ok(Value::Void)
    }

    fn parse_arguments(
        &mut self,
        command_line: Vec<Vec<u8>>,
        heap: &mut Heap,
        location: Location,
    ) -> Result<Value, Diagnostic> {
        let command_line = command_line
            .into_iter()
            .map(|bytes| host_text(bytes, location))
            .collect::<Result<Vec<_>, _>>()?;
        let mut command_line = command_line.into_iter();
        let mut positional = Vec::new();
        while let Some(name) = command_line.next() {
            if let Some(Argument { kind, destination }) = self.arguments.get(name.as_slice()) {
                let value = match kind {
                    ArgumentKind::Flag => Some(Value::Bool(true)),
                    ArgumentKind::Int => command_line
                        .next()
                        .and_then(|value| integer(&value, 10))
                        .map(Value::Int),
                    ArgumentKind::String => command_line.next().map(Value::string),
                };
                if let Some(value) = value {
                    destination.borrow_mut()[0] = value;
                }
            } else {
                positional.push(Value::string(name));
            }
        }
        let argc = length(positional.len(), location)?;
        let argv = Value::Array(Some(heap.block(positional)));
        let result = Value::Struct("args".to_owned(), heap.block(vec![Value::Int(argc), argv]));
        Ok(pointer(heap, Some(result)))
    }
}

fn strings(
    name: &str,
    args: &[Value],
    heap: &mut Heap,
    location: Location,
) -> Result<Value, Diagnostic> {
    Ok(match (name, args) {
        ("string_length", [Value::String(text)]) => Value::Int(length(text.len(), location)?),
        ("string_charat", [Value::String(text), Value::Int(index)]) => {
            let character = text
                .get(*index as usize)
                .ok_or_else(|| location.error("character index out of bounds"))?;
            Value::Char(ascii_char(*character, location)?)
        }
        ("string_join", [Value::String(left), Value::String(right)]) => {
            Value::string([left.as_ref(), right.as_ref()].concat())
        }
        ("string_sub", [Value::String(text), Value::Int(start), Value::Int(end)]) => {
            let text = text
                .get(*start as usize..*end as usize)
                .ok_or_else(|| location.error("substring range out of bounds"))?;
            Value::string(text)
        }
        ("string_equal", [Value::String(left), Value::String(right)]) => Value::Bool(left == right),
        ("string_compare", [Value::String(left), Value::String(right)]) => {
            Value::Int(left.cmp(right) as i32)
        }
        ("string_frombool", [Value::Bool(value)]) => Value::string(value.to_string().into_bytes()),
        ("string_fromint", [Value::Int(value)]) => Value::string(value.to_string().into_bytes()),
        ("string_fromchar", [Value::Char(value)]) => Value::string(if *value == 0 {
            Vec::new()
        } else {
            vec![*value]
        }),
        ("string_tolower", [Value::String(text)]) => Value::string(text.to_ascii_lowercase()),
        ("string_to_chararray", [Value::String(text)]) => {
            let chars = text
                .iter()
                .copied()
                .chain([0])
                .map(|byte| ascii_char(byte, location).map(Value::Char))
                .collect::<Result<_, _>>()?;
            Value::Array(Some(heap.block(chars)))
        }
        ("string_from_chararray", [Value::Array(array)]) => {
            let Some(array) = array else {
                return Err(location.error("character array has no null terminator"));
            };
            let array = array.borrow();
            let length = array
                .iter()
                .position(|value| matches!(value, Value::Char(0)))
                .ok_or_else(|| location.error("character array has no null terminator"))?;
            let text = array[..length]
                .iter()
                .map(|value| {
                    let Value::Char(character) = value else {
                        return Err(invalid_arguments(name, location));
                    };
                    ascii_char(*character, location)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Value::string(text)
        }
        ("string_terminated", [Value::Array(array), Value::Int(limit)]) => {
            let limit = usize::try_from(*limit)
                .map_err(|_| location.error("negative character array limit"))?;
            if limit > array.as_ref().map_or(0, |array| array.borrow().len()) {
                return Err(location.error("character array limit out of bounds"));
            }
            Value::Bool(array.as_ref().is_some_and(|array| {
                array.borrow()[..limit]
                    .iter()
                    .any(|value| matches!(value, Value::Char(0)))
            }))
        }
        ("char_ord", [Value::Char(character)]) => {
            Value::Int(i32::from(ascii_char(*character, location)?))
        }
        ("char_chr", [Value::Int(number)]) => {
            if !(0..=127).contains(number) {
                return Err(location.error("character code must be between 0 and 127"));
            }
            Value::Char(*number as u8)
        }
        _ => return Err(invalid_arguments(name, location)),
    })
}

fn length(length: usize, location: Location) -> Result<i32, Diagnostic> {
    i32::try_from(length).map_err(|_| location.error("value exceeds the C0 size limit"))
}

fn tokens(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|byte| byte.is_ascii_whitespace() || *byte == b'\x0b')
        .filter(|part| !part.is_empty())
}

fn parse_base(base: i32, location: Location) -> Result<u32, Diagnostic> {
    if !(2..=36).contains(&base) {
        return Err(location.error("integer base must be between 2 and 36"));
    }
    Ok(base as u32)
}

fn parse_int(text: &[u8], base: i32, location: Location) -> Result<Option<i32>, Diagnostic> {
    Ok(integer(text, parse_base(base, location)?))
}

fn integer(text: &[u8], base: u32) -> Option<i32> {
    let text = std::str::from_utf8(text)
        .ok()?
        .trim_start_matches(|ch: char| ch.is_ascii_whitespace() || ch == '\x0b');
    let (negative, digits) = if let Some(digits) = text.strip_prefix('-') {
        (true, digits)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    };
    let digits = if base == 16 {
        digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0X"))
            .unwrap_or(digits)
    } else {
        digits
    };
    if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(base)) {
        return None;
    }
    let magnitude = u32::from_str_radix(digits, base).ok()?;
    match (negative, base) {
        (true, _) if magnitude <= 1 << 31 => Some(magnitude.cast_signed().wrapping_neg()),
        (false, 16) => Some(magnitude.cast_signed()),
        (false, _) => i32::try_from(magnitude).ok(),
        _ => None,
    }
}

fn trim_line_ending(line: &mut Vec<u8>) {
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    }
}

fn without_nul(mut bytes: Vec<u8>) -> Vec<u8> {
    if let Some(index) = bytes.iter().position(|&byte| byte == 0) {
        bytes.truncate(index);
    }
    bytes
}

fn host_text(bytes: Vec<u8>, location: Location) -> Result<Vec<u8>, Diagnostic> {
    let text = without_nul(bytes);
    if text.is_ascii() {
        Ok(text)
    } else {
        Err(location.error("C0 text input must be ASCII"))
    }
}

fn ascii_char(byte: u8, location: Location) -> Result<u8, Diagnostic> {
    if byte.is_ascii() {
        Ok(byte)
    } else {
        Err(location.error("character code must be between 0 and 127"))
    }
}

fn pointer(heap: &mut Heap, value: Option<Value>) -> Value {
    Value::Pointer(value.map(|value| heap.block(vec![value])))
}

fn resource_id(value: &Value, location: Location) -> Result<usize, Diagnostic> {
    match value {
        Value::Pointer(None) => Err(location.error("resource handle is NULL")),
        Value::Pointer(Some(memory)) => match memory.borrow().first() {
            Some(Value::Resource(id)) => Ok(*id),
            _ => Err(location.error("invalid native resource handle")),
        },
        _ => Err(location.error("invalid native resource handle")),
    }
}
