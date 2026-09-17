//! Independent, version tolerant popup layout. Disabled rows retain their slot.
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleId {
    FiveHour,
    Weekly,
    ResetCredits,
    QuotaHistory,
    TokenActivity,
}
impl ModuleId {
    pub const ALL: [Self; 5] = [
        Self::FiveHour,
        Self::Weekly,
        Self::ResetCredits,
        Self::QuotaHistory,
        Self::TokenActivity,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::FiveHour => "5-hour quota",
            Self::Weekly => "Weekly quota",
            Self::ResetCredits => "Reset opportunities",
            Self::QuotaHistory => "Quota history",
            Self::TokenActivity => "Token activity",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleConfig {
    pub id: ModuleId,
    #[serde(default = "enabled")]
    pub visible: bool,
}
fn enabled() -> bool {
    true
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PanelConfig {
    pub modules: Vec<ModuleConfig>,
}
impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            modules: ModuleId::ALL
                .into_iter()
                .map(|id| ModuleConfig { id, visible: true })
                .collect(),
        }
    }
}
impl<'de> Deserialize<'de> for PanelConfig {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            modules: Vec<serde_json::Value>,
        }
        let raw = Raw::deserialize(d)?;
        let mut modules = Vec::new();
        for value in raw.modules {
            if let Ok(m) = serde_json::from_value::<ModuleConfig>(value) {
                if !modules.iter().any(|old: &ModuleConfig| old.id == m.id) {
                    modules.push(m);
                }
            }
        }
        for id in ModuleId::ALL {
            if !modules.iter().any(|m| m.id == id) {
                modules.push(ModuleConfig { id, visible: true });
            }
        }
        Ok(Self { modules })
    }
}
impl PanelConfig {
    pub fn enabled(&self, id: ModuleId) -> bool {
        self.modules.iter().any(|m| m.id == id && m.visible)
    }
    pub fn move_to(&mut self, from: usize, to: usize) {
        if from < self.modules.len() && to < self.modules.len() {
            let m = self.modules.remove(from);
            self.modules.insert(to, m);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_order_and_rollback() {
        let old: crate::config::schema::Config =
            toml::from_str("[general]\npanel_position_x=123\n[appearance]\n").unwrap();
        assert_eq!(old.panel, PanelConfig::default());
        assert_eq!(old.general.panel_position_x, Some(123));
        let mut draft = old.panel.clone();
        draft.modules[0].visible = false;
        draft.move_to(0, 4);
        let round: PanelConfig = toml::from_str(&toml::to_string(&draft).unwrap()).unwrap();
        assert_eq!(round, draft);
        assert_eq!(old.panel.modules[0].id, ModuleId::FiveHour);
        assert!(!round.modules[4].visible);
        let repaired:PanelConfig=toml::from_str("[[modules]]\nid='weekly'\nvisible=false\n[[modules]]\nid='future'\n[[modules]]\nid='weekly'").unwrap();
        assert_eq!(repaired.modules.len(), 5);
        assert!(!repaired.modules[0].visible);
    }
}
