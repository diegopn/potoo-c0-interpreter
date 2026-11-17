use crate::{
    Diagnostic, Location, RuntimeHost, TerminalArgument, TerminalResult,
    heap::{Heap, Value},
};

use super::{Resources, invalid_arguments};

pub(super) const HEADER: &str = "
    struct _win_st;
    typedef struct _win_st WINDOW;
    WINDOW* c_initscr();
    void c_cbreak();
    void c_noecho();
    void c_keypad(WINDOW* window, bool enabled);
    int c_getch();
    void c_addch(int character);
    void c_waddch(WINDOW* window, int character);
    void c_waddstr(WINDOW* window, string text);
    void c_wstandout(WINDOW* window);
    void c_wstandend(WINDOW* window);
    void c_delch();
    void c_erase();
    void c_werase(WINDOW* window);
    void c_wclear(WINDOW* window);
    void c_move(int y, int x);
    void c_wmove(WINDOW* window, int y, int x);
    void c_refresh();
    void c_wrefresh(WINDOW* window);
    int c_endwin();
    int c_curs_set(int visibility);
    WINDOW* c_subwin(WINDOW* window, int lines, int columns, int y, int x);
    void cc_wboldon(WINDOW* window);
    void cc_wboldoff(WINDOW* window);
    void cc_wdimon(WINDOW* window);
    void cc_wdimoff(WINDOW* window);
    void cc_wreverseon(WINDOW* window);
    void cc_wreverseoff(WINDOW* window);
    void cc_wunderon(WINDOW* window);
    void cc_wunderoff(WINDOW* window);
    int cc_highlight(int character);
    int cc_getx(WINDOW* window);
    int cc_gety(WINDOW* window);
    int cc_getmaxx(WINDOW* window);
    int cc_getmaxy(WINDOW* window);
    int cc_getbegx(WINDOW* window);
    int cc_getbegy(WINDOW* window);
    bool cc_key_is_enter(int key);
    bool cc_key_is_backspace(int key);
    bool cc_key_is_left(int key);
    bool cc_key_is_right(int key);
    bool cc_key_is_up(int key);
    bool cc_key_is_down(int key);
";

impl Resources {
    pub(super) fn terminal(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut impl RuntimeHost,
        heap: &mut Heap,
        location: Location,
    ) -> Result<Value, Diagnostic> {
        let args = args
            .iter()
            .map(|arg| {
                Ok(match arg {
                    Value::Int(value) => TerminalArgument::Int(*value),
                    Value::Bool(value) => TerminalArgument::Bool(*value),
                    Value::String(value) => TerminalArgument::Text(value),
                    Value::Pointer(None) => TerminalArgument::Window(None),
                    Value::Pointer(Some(window)) => match window.borrow().first() {
                        Some(Value::Window(id)) => TerminalArgument::Window(Some(*id)),
                        _ => return Err(location.error("invalid terminal handle")),
                    },
                    _ => return Err(invalid_arguments(name, location)),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let result = host
            .terminal(name, &args)
            .map_err(|error| location.error(format!("{name}: {error}")))?;
        let value = match (name, result) {
            ("c_initscr" | "c_subwin", TerminalResult::Window(window)) => {
                Value::Pointer(window.map(|id| {
                    self.windows
                        .entry(id)
                        .or_insert_with(|| heap.block(vec![Value::Window(id)]))
                        .clone()
                }))
            }
            (
                "c_getch" | "c_endwin" | "c_curs_set" | "cc_highlight" | "cc_getx" | "cc_gety"
                | "cc_getmaxx" | "cc_getmaxy" | "cc_getbegx" | "cc_getbegy",
                TerminalResult::Int(value),
            ) => Value::Int(value),
            (name, TerminalResult::Bool(value)) if name.starts_with("cc_key_is_") => {
                Value::Bool(value)
            }
            (
                "c_cbreak" | "c_noecho" | "c_keypad" | "c_addch" | "c_waddch" | "c_waddstr"
                | "c_wstandout" | "c_wstandend" | "c_delch" | "c_erase" | "c_werase" | "c_wclear"
                | "c_move" | "c_wmove" | "c_refresh" | "c_wrefresh" | "cc_wboldon" | "cc_wboldoff"
                | "cc_wdimon" | "cc_wdimoff" | "cc_wreverseon" | "cc_wreverseoff" | "cc_wunderon"
                | "cc_wunderoff",
                TerminalResult::Unit,
            ) => Value::Void,
            _ => {
                return Err(location.error(format!(
                    "{name}: terminal host returned an incompatible result"
                )));
            }
        };
        Ok(value)
    }
}
