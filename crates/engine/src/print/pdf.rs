//! A small PDF 1.4 writer: A4 pages, the standard Helvetica fonts (no embedding), text, lines and
//! filled rectangles. Enough for business documents, with no dependencies.

/// A4 in points.
pub const WIDTH: f32 = 595.0;
pub const HEIGHT: f32 = 842.0;

/// Glyph widths (1/1000 em) of Helvetica and Helvetica-Bold for ASCII 32..=126 (Adobe AFM).
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556,
    556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778,
    722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222,
    500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556,
    556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778,
    722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278,
    556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// RGB colour, components 0..=1.
#[derive(Clone, Copy)]
pub struct Rgb(pub f32, pub f32, pub f32);

pub const BLACK: Rgb = Rgb(0.2, 0.2, 0.2);
pub const GREY: Rgb = Rgb(0.4, 0.4, 0.4);

#[derive(Default)]
pub struct Pdf {
    pages: Vec<String>,
}

/// Text in WinAnsi (Latin-1 range); other characters become '?'. Escapes PDF string syntax.
fn encode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let b = match c {
            '\u{20}'..='\u{7e}' | '\u{a0}'..='\u{ff}' => c as u32 as u8,
            '\u{2013}' | '\u{2014}' => b'-',
            '\u{2019}' => b'\'',
            '\u{201c}' | '\u{201d}' => b'"',
            '\t' => b' ',
            _ => b'?',
        };
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

/// Width of `s` in points at `size`.
pub fn text_width(s: &str, size: f32, bold: bool) -> f32 {
    let table = if bold { &HELVETICA_BOLD } else { &HELVETICA };
    let units: u32 = s
        .chars()
        .map(|c| {
            let i = (c as u32).wrapping_sub(32) as usize;
            u32::from(table.get(i).copied().unwrap_or(556))
        })
        .sum();
    units as f32 * size / 1000.0
}

/// `s` cut (with "…" as "...") to fit `width`.
pub fn fit(s: &str, size: f32, bold: bool, width: f32) -> String {
    if text_width(s, size, bold) <= width {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        let candidate = format!("{out}{c}...");
        if text_width(&candidate, size, bold) > width {
            break;
        }
        out.push(c);
    }
    format!("{out}...")
}

impl Pdf {
    pub fn new() -> Self {
        Self { pages: vec![String::new()] }
    }

    pub fn new_page(&mut self) {
        self.pages.push(String::new());
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn cur(&mut self) -> &mut String {
        if self.pages.is_empty() {
            self.pages.push(String::new());
        }
        let last = self.pages.len() - 1;
        // In range: the list was made non-empty just above.
        &mut self.pages[last]
    }

    /// Text with its baseline `y` points from the top of the page.
    pub fn text(&mut self, x: f32, y: f32, size: f32, bold: bool, color: Rgb, s: &str) {
        let font = if bold { "F2" } else { "F1" };
        let body = String::from_utf8_lossy(&encode(s)).into_owned();
        let Rgb(r, g, b) = color;
        let op = format!("BT /{font} {size:.1} Tf {r:.3} {g:.3} {b:.3} rg {x:.2} {:.2} Td ({body}) Tj ET\n", HEIGHT - y);
        self.cur().push_str(&op);
    }

    /// Right-aligned text ending at `x`.
    pub fn text_right(&mut self, x: f32, y: f32, size: f32, bold: bool, color: Rgb, s: &str) {
        let w = text_width(s, size, bold);
        self.text(x - w, y, size, bold, color, s);
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Rgb) {
        let Rgb(r, g, b) = color;
        let op = format!("{r:.3} {g:.3} {b:.3} RG {width:.2} w {x1:.2} {:.2} m {x2:.2} {:.2} l S\n", HEIGHT - y1, HEIGHT - y2);
        self.cur().push_str(&op);
    }

    /// A filled rectangle with its top-left corner `y` points from the top.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb) {
        let Rgb(r, g, b) = color;
        let op = format!("{r:.3} {g:.3} {b:.3} rg {x:.2} {:.2} {w:.2} {h:.2} re f\n", HEIGHT - y - h);
        self.cur().push_str(&op);
    }

    /// The finished file.
    pub fn finish(self) -> Vec<u8> {
        let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets: Vec<usize> = Vec::new();
        let n = self.pages.len();
        // Objects: 1 catalog, 2 pages, 3 Helvetica, 4 Helvetica-Bold, then page + content pairs.
        let page_ids: Vec<usize> = (0..n).map(|i| 5 + i * 2).collect();
        let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
            offsets.push(out.len());
            let id = offsets.len();
            out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        };
        obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
        let kids: Vec<String> = page_ids.iter().map(|id| format!("{id} 0 R")).collect();
        obj(&mut out, format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).as_bytes());
        obj(&mut out, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
        obj(&mut out, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>");
        for (i, content) in self.pages.iter().enumerate() {
            let content_id = 6 + i * 2;
            obj(
                &mut out,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {content_id} 0 R >>"
                )
                .as_bytes(),
            );
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(content.as_bytes());
            stream.extend_from_slice(b"\nendstream");
            obj(&mut out, &stream);
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
        for o in &offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_valid_looking_pdf() {
        let mut p = Pdf::new();
        p.text(50.0, 60.0, 12.0, true, BLACK, "Invoice (draft) \\ 100%");
        p.line(50.0, 70.0, 300.0, 70.0, 0.5, GREY);
        p.new_page();
        p.text_right(545.0, 60.0, 10.0, false, BLACK, "Rp 1.000");
        let bytes = p.finish();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("%PDF-1.4"));
        assert!(s.contains("/Count 2"));
        assert!(s.contains("\\(draft\\) \\\\ 100%"));
        assert!(s.trim_end().ends_with("%%EOF"));
    }

    #[test]
    fn widths_and_fitting() {
        assert!((text_width("W", 10.0, false) - 9.44).abs() < 0.01);
        let f = fit("A very long description that will not fit", 9.0, false, 80.0);
        assert!(f.ends_with("...") && text_width(&f, 9.0, false) <= 80.0);
    }
}
