//! Independent taskbar ring and number styling, in device-independent pixels.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberWeight {
    Regular,
    #[default]
    Semibold,
    Bold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Appearance {
    pub ring_color: String,
    /// `auto` follows the taskbar theme; otherwise #RRGGBB.
    pub number_color: String,
    pub ring_size: i32,
    pub ring_thickness: i32,
    pub ring_x: i32,
    /// Offset from vertical center; clamped inside the taskbar at render time.
    pub ring_y: i32,
    pub number_size: i32,
    pub number_x: i32,
    pub number_y: i32,
    pub number_weight: NumberWeight,
    pub symbol_percent: i32,
}

// Decode each setting independently: malformed input must not discard credentials
// or valid sibling preferences when a user edits the TOML by hand.
impl<'de> Deserialize<'de> for Appearance {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = toml::Value::deserialize(deserializer)?;
        let mut style = Self::default();
        macro_rules! field {
            ($name:ident) => {
                if let Some(value) = raw.get(stringify!($name)) {
                    if let Ok(parsed) = Deserialize::deserialize(value.clone()) {
                        style.$name = parsed;
                    }
                }
            };
        }
        field!(ring_color);
        field!(number_color);
        field!(ring_size);
        field!(ring_thickness);
        field!(ring_x);
        field!(ring_y);
        field!(number_size);
        field!(number_x);
        field!(number_y);
        field!(number_weight);
        field!(symbol_percent);
        style.normalize();
        Ok(style)
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            ring_color: "#FF3774".into(),
            number_color: "auto".into(),
            ring_size: 28,
            ring_thickness: 4,
            ring_x: 8,
            ring_y: 0,
            number_size: 20,
            number_x: 43,
            number_y: 0,
            number_weight: NumberWeight::Semibold,
            symbol_percent: 60,
        }
    }
}

pub fn rgb(value: &str) -> Option<[u8; 3]> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

impl Appearance {
    pub fn normalize(&mut self) {
        if rgb(&self.ring_color).is_none() {
            self.ring_color = Self::default().ring_color;
        }
        if self.number_color != "auto" && rgb(&self.number_color).is_none() {
            self.number_color = "auto".into();
        }
        self.ring_size = self.ring_size.clamp(12, 44);
        self.ring_thickness = self
            .ring_thickness
            .clamp(1, (self.ring_size / 2 - 1).min(12));
        self.ring_x = self.ring_x.clamp(0, 300);
        self.ring_y = self.ring_y.clamp(-20, 20);
        self.number_size = self.number_size.clamp(10, 40);
        self.number_x = self.number_x.clamp(0, 300);
        self.number_y = self.number_y.clamp(-20, 20);
        self.symbol_percent = self.symbol_percent.clamp(40, 100);
    }

    pub fn validate(&self) -> bool {
        let mut normalized = self.clone();
        normalized.normalize();
        normalized == *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_and_bad_colors_do_not_reset_other_preferences() {
        let mut style = Appearance {
            ring_color: "#中文".into(),
            number_color: "#1a2B3c".into(),
            ring_size: 12,
            ring_thickness: 12,
            number_x: 87,
            number_size: 999,
            ..Default::default()
        };
        assert!(!style.validate());
        style.normalize();
        assert_eq!(style.ring_thickness, 5);
        assert_eq!(style.number_size, 40);
        assert_eq!(style.number_x, 87);
        assert_eq!(rgb(&style.number_color), Some([26, 43, 60]));
        assert!(style.validate());
        assert_eq!(
            toml::from_str::<Appearance>(&toml::to_string(&style).unwrap()).unwrap(),
            style
        );
    }
}
