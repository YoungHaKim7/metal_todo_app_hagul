mod gui;

/// Hangul composition — folds the jamo stream the 2-Set Korean keyboard
/// emits into composed syllables (the GUI is not a text-input client, so
/// macOS never does it for us; see the module docs).
mod hangul;

const W: f64 = 560.0;
const H: f64 = 640.0;

struct Todo {
    tasks: Vec<(String, bool)>,
    selected: usize,
    input: String,
}

impl Todo {
    fn new() -> Todo {
        let tasks = vec![
            ("Write a Metal shader".to_string(), false),
            ("Ship the ObjC ABI layer".to_string(), true),
            ("Buy a magical keyboard".to_string(), false),
        ];
        Todo {
            tasks,
            selected: 0,
            input: String::new(),
        }
    }
}

fn main() {
    let mut todo = Todo::new();
    let app = gui::App::new_points("081 - Todo App", W, H);
    app.run(move |f| {
        f.set_clear(gui::Color::rgb(0.10, 0.10, 0.14));

        let margin = 20.0;
        let row_h = 44.0;
        let input_h = 56.0;
        // First task row. The header (y 24..60) and then the input line
        // (56 high, 8 above this) sit above it without overlapping either.
        let top = 132.0;

        // Header.
        f.text(margin, 24.0, 5.0, "TODO", gui::Color::WHITE);
        let done_count = todo.tasks.iter().filter(|t| t.1).count();
        let stats = format!("{} DONE / {} TOTAL", done_count, todo.tasks.len());
        f.text(
            W as f32 - margin - f.text_width(2.5, &stats),
            34.0,
            2.5,
            &stats,
            gui::Color::GRAY,
        );

        // Input line.
        let in_rect = (
            margin,
            top - input_h - 8.0,
            W as f32 - 2.0 * margin,
            input_h,
        );
        f.rect(
            in_rect.0,
            in_rect.1,
            in_rect.2,
            in_rect.3,
            gui::Color::rgb(0.16, 0.16, 0.22),
        );
        f.frame_rect(
            in_rect.0,
            in_rect.1,
            in_rect.2,
            in_rect.3,
            2.0,
            gui::Color::rgb(0.3, 0.42, 0.8),
        );
        let prompt = "> ";
        let cursor = if (f.time % 1.0) < 0.5 { "_" } else { " " };
        let line = format!("{prompt}{}{cursor}", todo.input);
        // Shrink the input line to the box: 40 characters of Korean are
        // about twice as wide as 40 of Latin, and the composer means both
        // can sit in the same input.
        let mut input_size = 3.0;
        while input_size > 1.0 && f.text_width(input_size, &line) > in_rect.2 - 24.0 {
            input_size -= 0.25;
        }
        f.text(
            in_rect.0 + 12.0,
            in_rect.1 + 16.0,
            input_size,
            &line,
            gui::Color::WHITE,
        );

        // Task rows.
        let mut click_row: Option<usize> = None;
        for (i, (task, done)) in todo.tasks.iter().enumerate() {
            let y = top + i as f32 * (row_h + 6.0);
            if y + row_h > H as f32 - 56.0 {
                break;
            }
            let selected = i == todo.selected;
            let hovered = {
                let (mx, my) = f.mouse();
                mx >= margin && mx <= W as f32 - margin && my >= y && my <= y + row_h
            };
            let bg = if selected {
                gui::Color::rgb(0.22, 0.30, 0.50)
            } else if hovered {
                gui::Color::rgb(0.18, 0.18, 0.24)
            } else {
                gui::Color::rgb(0.15, 0.15, 0.20)
            };
            f.rect(margin, y, W as f32 - 2.0 * margin, row_h, bg);

            // Checkbox.
            let cb = 20.0;
            f.rect(
                margin + 12.0,
                y + (row_h - cb) * 0.5,
                cb,
                cb,
                if *done {
                    gui::Color::rgb(0.25, 0.85, 0.45)
                } else {
                    gui::Color::rgb(0.3, 0.3, 0.38)
                },
            );
            if *done {
                f.text(
                    margin + 14.0,
                    y + (row_h - cb) * 0.5 - 1.0,
                    3.0,
                    "X",
                    gui::Color::BLACK,
                );
            }

            // Task text + strikethrough when done.
            let color = if *done {
                gui::Color::GRAY
            } else {
                gui::Color::WHITE
            };
            f.text(
                margin + 46.0,
                y + (row_h - 7.0 * 2.5) * 0.5,
                2.5,
                task,
                color,
            );
            if *done {
                let tw = f.text_width(2.5, task);
                f.line(
                    margin + 46.0,
                    y + row_h * 0.5,
                    margin + 46.0 + tw,
                    y + row_h * 0.5,
                    2.0,
                    gui::Color::GRAY,
                );
            }

            if f.input.mouse_pressed() && hovered {
                click_row = Some(i);
            }
        }
        if let Some(i) = click_row {
            todo.selected = i;
            if let Some(t) = todo.tasks.get_mut(i) {
                t.1 = !t.1;
            }
        }

        // Empty state.
        if todo.tasks.is_empty() {
            f.text_center(
                f.width * 0.5,
                top + 60.0,
                3.0,
                "NOTHING TO DO. TYPE A TASK AND PRESS ENTER.",
                gui::Color::GRAY,
            );
        }

        // Keyboard + pasteboard text: every printable character is
        // accepted — lowercase letters, digits, punctuation, Korean jamo
        // and composed syllables — then the Hangul composer folds jamo
        // sequences into syllables (ㄱ ㅏ ㄴ ㅏ ends up as 가나). The
        // 40-character cap counts characters, not bytes, so Korean text
        // gets the same room as Latin.
        for ch in f.input.text().chars() {
            if !ch.is_control() && todo.input.chars().count() < 40 {
                todo.input.push(ch);
            }
        }
        todo.input = hangul::compose(&todo.input);

        // ⌘C copies and ⌘X cuts the pending input (⌘V is handled by the
        // GUI layer and lands on `f.input.text()` above). Nothing is
        // copied while the input is empty — that would wipe the user's
        // clipboard.
        if f.input.cmd() && f.input.key_pressed(gui::key::C) && !todo.input.is_empty() {
            f.set_clipboard(&todo.input);
        }
        if f.input.cmd() && f.input.key_pressed(gui::key::X) && !todo.input.is_empty() {
            f.set_clipboard(&todo.input);
            todo.input.clear();
        }
        if f.input.key_pressed(gui::key::ENTER) {
            if !todo.input.trim().is_empty() {
                todo.tasks.insert(0, (todo.input.clone(), false));
                todo.selected = 0;
            }
            todo.input.clear();
        }
        if f.input.key_pressed(gui::key::BACKSPACE) {
            if todo.input.is_empty() {
                if !todo.tasks.is_empty() && todo.selected < todo.tasks.len() {
                    todo.tasks.remove(todo.selected);
                    todo.selected = todo.selected.saturating_sub(1);
                }
            } else {
                todo.input.pop();
            }
        }
        if f.input.key_pressed(gui::key::UP) && !todo.tasks.is_empty() {
            todo.selected = todo.selected.saturating_sub(1);
        }
        if f.input.key_pressed(gui::key::DOWN)
            && !todo.tasks.is_empty()
            && todo.selected < todo.tasks.len() - 1
        {
            todo.selected += 1;
        }

        // SPACE toggles the selected task — but only while the input is
        // empty, so it can still be typed into a task being written.
        if f.input.key_pressed(gui::key::SPACE)
            && todo.input.is_empty()
            && let Some(t) = todo.tasks.get_mut(todo.selected)
        {
            t.1 = !t.1;
        }

        if f.input.key_pressed(gui::key::D)
            && todo.input.is_empty()
            && !todo.tasks.is_empty()
            && todo.selected < todo.tasks.len()
        {
            todo.tasks.remove(todo.selected);
            todo.selected = todo.selected.saturating_sub(1);
        }

        f.text(
            margin,
            H as f32 - 36.0,
            2.0,
            "TYPE + ENTER ADD - UP/DOWN SELECT - SPACE TOGGLE - D DELETE",
            gui::Color::GRAY,
        );
        f.text(
            margin,
            H as f32 - 20.0,
            2.0,
            "CMD+C COPY - CMD+V PASTE - CMD+X CUT - LOWERCASE + KOREAN OK",
            gui::Color::GRAY,
        );
    });
}
