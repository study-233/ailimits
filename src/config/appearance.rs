//! Independent taskbar ring and number styling, in device-independent pixels.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayStyle {
    #[default]
    Rings,
    Bars,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaPeriods {
    #[default]
    Weekly,
    FiveHours,
    Both,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberWeight {
    Regular,
    #[default]
    Semibold,
    Bold,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayDisplay {
    #[default]
    Auto,
    Ring,
    Number,
    RingState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Appearance {
    pub tray_display: TrayDisplay,
    pub display_style: DisplayStyle,
    pub periods: QuotaPeriods,
    pub session_color: String,
    pub bar_width: i32,
    pub bar_thickness: i32,
    pub bar_x: i32,
    pub bar_y: i32,
    pub bar_number_x: i32,
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
        field!(tray_display);
        field!(ring_color);
        field!(display_style);
        field!(periods);
        field!(session_color);
        field!(bar_width);
        field!(bar_thickness);
        field!(bar_x);
        field!(bar_y);
        field!(bar_number_x);
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
            tray_display: TrayDisplay::Auto,
            display_style: DisplayStyle::Rings,
            periods: QuotaPeriods::Weekly,
            session_color: "#30C8F0".into(),
            bar_width: 88,
            bar_thickness: 8,
            bar_x: 112,
            bar_y: 0,
            bar_number_x: 8,
            ring_color: "#FF3774".into(),
            number_color: "auto".into(),
            ring_size: 28,
            ring_thickness: 4,
            ring_x: 6,
            ring_y: 0,
            number_size: 20,
            number_x: 41,
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
        if rgb(&self.session_color).is_none() {
            self.session_color = Self::default().session_color;
        }
        self.bar_width = self.bar_width.clamp(32, 200);
        self.bar_thickness = self.bar_thickness.clamp(2, 16);
        self.bar_x = self.bar_x.clamp(0, 300);
        self.bar_y = self.bar_y.clamp(-20, 20);
        self.bar_number_x = self.bar_number_x.clamp(0, 300);
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
    fn new_fields_fail_independently_and_legacy_styles_survive() {
        let old: Appearance =
            toml::from_str("ring_color = '#123456'\nnumber_x = 99\nnumber_size = 24").unwrap();
        assert_eq!(old.display_style, DisplayStyle::Rings);
        assert_eq!(old.tray_display, TrayDisplay::Auto);
        for tray_display in [
            TrayDisplay::Auto,
            TrayDisplay::Ring,
            TrayDisplay::Number,
            TrayDisplay::RingState,
        ] {
            let styled = Appearance {
                tray_display,
                ..old.clone()
            };
            assert_eq!(
                toml::from_str::<Appearance>(&toml::to_string(&styled).unwrap()).unwrap(),
                styled
            );
        }
        assert_eq!(old.periods, QuotaPeriods::Weekly);
        assert_eq!(old.number_x, 99);
        let broken: Appearance = toml::from_str("display_style = 'future'\nperiods = 42\nsession_color = 'bad'\nbar_width = -1\nring_color = '#123456'\nnumber_x = 99").unwrap();
        assert_eq!(broken.display_style, DisplayStyle::Rings);
        assert_eq!(broken.periods, QuotaPeriods::Weekly);
        assert_eq!(broken.bar_width, 32);
        assert_eq!(broken.ring_color, old.ring_color);
        assert_eq!(broken.number_x, old.number_x);
        for display_style in [DisplayStyle::Rings, DisplayStyle::Bars] {
            for periods in [
                QuotaPeriods::Weekly,
                QuotaPeriods::FiveHours,
                QuotaPeriods::Both,
            ] {
                let style = Appearance {
                    display_style,
                    periods,
                    ..old.clone()
                };
                assert_eq!(
                    toml::from_str::<Appearance>(&toml::to_string(&style).unwrap()).unwrap(),
                    style
                );
            }
        }
    }
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
