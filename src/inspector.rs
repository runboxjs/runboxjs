/// Inspector DOM — protocolo de inspección de elementos del browser.
/// El lado Rust define los mensajes; el lado JS ejecuta la inspección real.
use serde::{Deserialize, Serialize};

// ── Requests de inspección ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InspectRequest {
    /// Inspeccionar el elemento en estas coordenadas del viewport.
    AtPoint { x: f64, y: f64 },
    /// Inspeccionar el elemento que coincida con el selector CSS.
    BySelector { selector: String },
    /// Inspeccionar el elemento con este ID interno del browser.
    ById { id: u64 },
    /// Desactivar el inspector y limpiar el highlight.
    Dismiss,
}

// ── Respuesta del browser ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectedNode {
    /// ID interno del browser para referencias posteriores.
    pub id: u64,
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_attr: Option<String>,
    pub classes: Vec<String>,
    pub attributes: Vec<Attribute>,
    pub box_model: BoxModel,
    pub styles: ComputedStyles,
    pub children: Vec<ChildSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner_text: Option<String>,
    pub source: Option<SourceLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute {
    pub name: String,
    pub value: String,
}

/// Modelo de caja CSS (px).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxModel {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub margin: Spacing,
    pub padding: Spacing,
    pub border: Spacing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spacing {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

/// Estilos computados relevantes del elemento.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedStyles {
    /// Reglas CSS aplicadas (origen, selector, propiedades).
    pub rules: Vec<StyleRule>,
    /// Propiedades computadas finales.
    pub computed: Vec<StyleProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleRule {
    pub selector: String,
    pub origin: StyleOrigin,
    pub properties: Vec<StyleProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StyleOrigin {
    Author,
    UserAgent,
    Inline,
    Animation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleProperty {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub important: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildSummary {
    pub id: u64,
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_attr: Option<String>,
    pub classes: Vec<String>,
}

/// Ubicación en el código fuente (si hay sourcemaps).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

// ── Highlight overlay ─────────────────────────────────────────────────────────

/// Instrucciones para pintar el overlay de highlight en el browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightOverlay {
    pub node_id: u64,
    pub content_box: OverlayRect,
    pub padding_box: OverlayRect,
    pub border_box: OverlayRect,
    pub margin_box: OverlayRect,
    pub tooltip: HighlightTooltip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: OverlayColor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f64,
}

impl OverlayColor {
    pub fn content() -> Self {
        Self {
            r: 111,
            g: 168,
            b: 220,
            a: 0.66,
        }
    }
    pub fn padding() -> Self {
        Self {
            r: 147,
            g: 196,
            b: 125,
            a: 0.55,
        }
    }
    pub fn border() -> Self {
        Self {
            r: 255,
            g: 229,
            b: 153,
            a: 0.66,
        }
    }
    pub fn margin() -> Self {
        Self {
            r: 246,
            g: 178,
            b: 107,
            a: 0.66,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightTooltip {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub size: String, // "320 × 48"
}

impl HighlightOverlay {
    /// Genera el overlay a partir de un nodo inspeccionado.
    pub fn from_node(node: &InspectedNode) -> Self {
        let bm = &node.box_model;
        let tag = node.tag.clone();
        let id = node.id_attr.clone();
        let classes = node.classes.clone();
        let size = format!("{:.0} × {:.0}", bm.width, bm.height);

        // content box
        let cx = bm.x + bm.padding.left + bm.border.left;
        let cy = bm.y + bm.padding.top + bm.border.top;
        let cw = bm.width - bm.padding.left - bm.padding.right - bm.border.left - bm.border.right;
        let ch = bm.height - bm.padding.top - bm.padding.bottom - bm.border.top - bm.border.bottom;

        // padding box (content + padding)
        let px = bm.x + bm.border.left;
        let py = bm.y + bm.border.top;
        let pw = bm.width - bm.border.left - bm.border.right;
        let ph = bm.height - bm.border.top - bm.border.bottom;

        // margin box (full box + margin)
        let mx = bm.x - bm.margin.left;
        let my = bm.y - bm.margin.top;
        let mw = bm.width + bm.margin.left + bm.margin.right;
        let mh = bm.height + bm.margin.top + bm.margin.bottom;

        Self {
            node_id: node.id,
            content_box: OverlayRect {
                x: cx,
                y: cy,
                width: cw,
                height: ch,
                color: OverlayColor::content(),
            },
            padding_box: OverlayRect {
                x: px,
                y: py,
                width: pw,
                height: ph,
                color: OverlayColor::padding(),
            },
            border_box: OverlayRect {
                x: bm.x,
                y: bm.y,
                width: bm.width,
                height: bm.height,
                color: OverlayColor::border(),
            },
            margin_box: OverlayRect {
                x: mx,
                y: my,
                width: mw,
                height: mh,
                color: OverlayColor::margin(),
            },
            tooltip: HighlightTooltip {
                tag,
                id,
                classes,
                size,
            },
        }
    }
}

// ── 6.3 Network Profiler & DevTools ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEvent {
    pub request_id: String,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub duration_ms: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PerformanceTimeline {
    pub events: Vec<PerformanceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceEntry {
    pub name: String,
    pub entry_type: String,
    pub start_time: f64,
    pub duration: f64,
}

// ── Session del inspector ─────────────────────────────────────────────────────

/// Estado de la sesión de inspección activa y DevTools.
#[derive(Debug, Default)]
pub struct InspectorSession {
    pub active: bool,
    pub selected: Option<InspectedNode>,
    pub history: Vec<InspectedNode>,
    pub network_events: Vec<NetworkEvent>,
    pub timeline: PerformanceTimeline,
}

impl InspectorSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn activate(&mut self) {
        self.active = true;
    }
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    pub fn set_node(&mut self, node: InspectedNode) {
        self.history.push(node.clone());
        self.selected = Some(node);
    }

    /// Retorna el overlay para el nodo actualmente seleccionado.
    pub fn overlay(&self) -> Option<HighlightOverlay> {
        self.selected.as_ref().map(HighlightOverlay::from_node)
    }

    /// Serializa el nodo seleccionado a JSON para el browser.
    pub fn selected_json(&self) -> String {
        serde_json::to_string(&self.selected).unwrap_or_default()
    }

    pub fn overlay_json(&self) -> String {
        serde_json::to_string(&self.overlay()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_color() {
        let content = OverlayColor::content();
        assert_eq!(content.r, 111);
        let padding = OverlayColor::padding();
        assert_eq!(padding.r, 147);
        let border = OverlayColor::border();
        assert_eq!(border.r, 255);
        let margin = OverlayColor::margin();
        assert_eq!(margin.r, 246);
    }

    #[test]
    fn test_inspector_session() {
        let mut session = InspectorSession::new();
        assert!(!session.active);
        session.activate();
        assert!(session.active);
        session.deactivate();
        assert!(!session.active);

        let node = InspectedNode {
            id: 1,
            tag: "div".into(),
            id_attr: Some("app".into()),
            classes: vec!["container".into()],
            attributes: vec![],
            box_model: BoxModel {
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 100.0,
                margin: Spacing {
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
                padding: Spacing {
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
                border: Spacing {
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
            },
            styles: ComputedStyles {
                rules: vec![],
                computed: vec![],
            },
            children: vec![],
            inner_text: None,
            source: None,
        };

        session.set_node(node);
        assert_eq!(session.history.len(), 1);
        assert!(session.selected.is_some());

        let overlay = session.overlay();
        assert!(overlay.is_some());
        assert_eq!(overlay.unwrap().node_id, 1);
    }
}
