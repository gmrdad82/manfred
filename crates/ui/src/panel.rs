use pfx_editor_shell::focus::Code;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Panel {
    A,
    B1,
    B2,
    B3,
    #[default]
    C1,
    C2,
    C3,
    D1,
}

impl Panel {
    pub const ALL: [Panel; 8] = [
        Panel::A,
        Panel::B1,
        Panel::B2,
        Panel::B3,
        Panel::C1,
        Panel::C2,
        Panel::C3,
        Panel::D1,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Panel::A => "A",
            Panel::B1 => "B1",
            Panel::B2 => "B2",
            Panel::B3 => "B3",
            Panel::C1 => "C1",
            Panel::C2 => "C2",
            Panel::C3 => "C3",
            Panel::D1 => "D1",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Panel::A => "recipe",
            Panel::B1 => "outliner",
            Panel::B2 => "cameras",
            Panel::B3 => "lights & sky",
            Panel::C1 => "viewport",
            Panel::C2 => "properties",
            Panel::C3 => "render",
            Panel::D1 => "status",
        }
    }
}

impl Code for Panel {
    fn code(self) -> &'static str {
        Panel::code(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Key;
    use pfx_editor_shell::focus::{Keys, Press, from_code};

    fn run(keys: &[Key]) -> Vec<Press<Panel>> {
        let mut state = Keys::default();
        keys.iter()
            .map(|key| state.press(&Panel::ALL, *key))
            .collect()
    }

    #[test]
    fn a_letter_then_a_digit_jumps() {
        assert_eq!(
            run(&[Key::B, Key::Num1]),
            vec![Press::Pending, Press::Focus(Panel::B1)]
        );
        assert_eq!(
            run(&[Key::C, Key::Num3]),
            vec![Press::Pending, Press::Focus(Panel::C3)]
        );
        assert_eq!(
            run(&[Key::D, Key::Num1]),
            vec![Press::Pending, Press::Focus(Panel::D1)]
        );
    }

    #[test]
    fn the_top_bar_answers_its_letter() {
        assert_eq!(run(&[Key::A]), vec![Press::Focus(Panel::A)]);
    }

    #[test]
    fn a_broken_sequence_forgets_its_letter() {
        assert_eq!(
            run(&[Key::B, Key::X, Key::Num1]),
            vec![
                Press::Pending,
                Press::Other(Key::X),
                Press::Other(Key::Num1)
            ]
        );
        assert_eq!(
            run(&[Key::D, Key::Num2, Key::Num1]),
            vec![Press::Pending, Press::Pending, Press::Other(Key::Num1)]
        );
        assert_eq!(run(&[Key::Num1]), vec![Press::Other(Key::Num1)]);
    }

    #[test]
    fn every_code_names_its_panel() {
        for panel in Panel::ALL {
            assert_eq!(from_code(&Panel::ALL, panel.code()), Some(panel));
        }
    }
}
