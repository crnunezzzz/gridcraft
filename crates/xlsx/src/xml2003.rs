//! XML Spreadsheet 2003 (SpreadsheetML 2003) import: the XML format Excel 2002 and 2003 saved as
//! "XML Spreadsheet", and which many web systems still produce for "Export to Excel", usually under
//! an `.xls` name. Clean-room from Microsoft's public *XML Spreadsheet Reference* (Office XP SDK);
//! all test files are built in Rust.
//!
//! A `Workbook` (namespace `urn:schemas-microsoft-com:office:spreadsheet`, attributes prefixed
//! `ss:`) holds `Styles` and one `Worksheet` per sheet. A worksheet's `Table` lists `Column`s
//! (width in points) and `Row`s (height in points) of `Cell`s; `ss:Index` (1-based) skips ahead,
//! `ss:Span` repeats a column or row, `ss:MergeAcross`/`ss:MergeDown` merge. A cell's `Data` has an
//! `ss:Type` (`Number`, `String`, `Boolean`, `DateTime`, `Error`) and its formula is
//! `ss:Formula`, in R1C1 notation; the caller supplies the R1C1 → A1 conversion.
//! `WorksheetOptions` (namespace `urn:schemas-microsoft-com:office:excel`) carries frozen panes,
//! visibility, right-to-left and gridlines.
//!
//! Read: sheets, values, formulas (with their saved results), cell styles (number format, font,
//! fill, borders, alignment, protection, inherited through `ss:Parent`), merges, column widths,
//! row heights, hidden rows and columns, frozen panes, hidden sheets, right-to-left, gridlines.
//! Not read: comments, named ranges, conditional formatting, validation, autofilter, print settings.

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{
    BorderLine, BorderStyle, Cell, Color, Fill, Formula, HAlign, LineInfo, NumFmt, PatternType, ReadingOrder, Sheet, Style, StyleId, Underline,
    VAlign, VertAlign, Visibility, Workbook,
};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use crate::tables::pt_to_px;
use crate::{IoError, ReadReport};

const NS: &str = "urn:schemas-microsoft-com:office:spreadsheet";
const MAX_BYTES: usize = 512 << 20;
const MAX_CELLS: u64 = 5_000_000;
const MAX_TEXT: u64 = 256 << 20;
const MAX_CELL_TEXT: usize = 32_767;
const MAX_SHEETS: usize = 1024;
const MAX_STYLES: usize = 64_000;
const MAX_MERGES: usize = 200_000;
/// Row and column settings kept from one file; `ss:Span` can name a million rows at a time.
const MAX_LINES: usize = 4_000_000;

/// Whether the bytes are an XML Spreadsheet 2003 document: XML whose start declares the
/// spreadsheet namespace on a `Workbook` element.
pub(crate) fn is_xml2003(bytes: &[u8]) -> bool {
    let head = crate::decode_text(bytes.get(..bytes.len().min(8192)).unwrap_or(bytes));
    let t = head.trim_start_matches('\u{feff}').trim_start();
    t.starts_with('<') && t.contains(NS) && (t.contains("<Workbook") || t.contains(":Workbook"))
}

/// Reads an XML Spreadsheet 2003 document. `r1c1_to_a1` rewrites a formula's R1C1 references as
/// A1 for the cell it is in (the formula is passed without its `=`).
pub fn read_xml2003(bytes: &[u8], r1c1_to_a1: &dyn Fn(&str, CellRef) -> String) -> Result<(Workbook, ReadReport), IoError> {
    if bytes.len() > MAX_BYTES {
        return Err(IoError::TooLarge(format!("the XML spreadsheet is larger than {} MB", MAX_BYTES >> 20)));
    }
    // UTF-16 files are decoded first; the reader below works on UTF-8.
    let owned;
    let text: &[u8] = if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        owned = crate::decode_text(bytes);
        owned.as_bytes()
    } else {
        bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
    };
    let mut p = Parser::new(r1c1_to_a1);
    p.run(text)?;
    if p.wb.sheets.is_empty() {
        return Err(IoError::Format("the XML spreadsheet has no worksheets".into()));
    }
    Ok((p.wb, p.report))
}

fn xml_error(e: impl std::fmt::Display) -> IoError {
    IoError::Xml(format!("XML Spreadsheet: {e}"))
}

/// The local part of a (possibly prefixed) name.
fn local(name: &[u8]) -> &[u8] {
    name.iter().rposition(|&b| b == b':').map_or(name, |i| name.get(i + 1..).unwrap_or_default())
}

/// The element's attributes by local name (`ss:Index` → `Index`); namespaces are not checked, as
/// writers differ in which prefix (or none) they put on attributes.
fn attrs(e: &BytesStart<'_>) -> Result<HashMap<String, String>, IoError> {
    let mut out = HashMap::new();
    for a in e.attributes() {
        let a = a.map_err(xml_error)?;
        let key = String::from_utf8_lossy(local(a.key.as_ref())).into_owned();
        if key == "xmlns" || a.key.as_ref().starts_with(b"xmlns") {
            continue;
        }
        let value = a.normalized_value(XmlVersion::Explicit1_0).map_err(xml_error)?;
        if value.len() > MAX_CELL_TEXT * 4 {
            return Err(IoError::TooLarge("an XML Spreadsheet attribute is too long".into()));
        }
        out.insert(key, value.into_owned());
    }
    Ok(out)
}

fn flag(a: &HashMap<String, String>, key: &str) -> bool {
    a.get(key).is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// A 1-based position attribute → 0-based. A position past `max` is `max` itself, outside the
/// grid, so what it holds is skipped rather than moved somewhere else; a missing or invalid one is `None`.
fn index(a: &HashMap<String, String>, key: &str, max: u32) -> Option<u32> {
    let n = a.get(key)?.trim().parse::<u64>().ok().filter(|n| *n >= 1)?;
    Some(u32::try_from(n - 1).unwrap_or(max).min(max))
}

fn count(a: &HashMap<String, String>, key: &str) -> u32 {
    a.get(key).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0)
}

fn points(a: &HashMap<String, String>, key: &str) -> Option<f64> {
    a.get(key)?.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0 && *v <= 4096.0)
}

struct SheetState {
    sheet: Sheet,
    row: u32,
    /// How many more rows the open `Row` stands for (`ss:Span`).
    row_span: u32,
    col: u32,
    /// The next column a `Column` element describes.
    column: u32,
    freeze: bool,
    split_rows: u32,
    split_cols: u32,
}

struct CellState {
    at: CellRef,
    style: Option<String>,
    formula: Option<String>,
    across: u32,
    down: u32,
    kind: Option<String>,
    text: String,
    /// The stack depth of the cell's own `Data` element while it is open.
    data: Option<usize>,
}

struct Parser<'a> {
    r1c1_to_a1: &'a dyn Fn(&str, CellRef) -> String,
    wb: Workbook,
    report: ReadReport,
    stack: Vec<Vec<u8>>,
    /// Resolved styles by `ss:ID`.
    styles: HashMap<String, Style>,
    ids: HashMap<String, StyleId>,
    /// The style being read inside `Styles`.
    style: Option<(String, Style)>,
    sheet: Option<SheetState>,
    cell: Option<CellState>,
    /// Text of the `WorksheetOptions` element being read.
    option: Option<(Vec<u8>, String)>,
    cells: u64,
    text: u64,
    merges: usize,
    lines: usize,
}

impl<'a> Parser<'a> {
    fn new(r1c1_to_a1: &'a dyn Fn(&str, CellRef) -> String) -> Self {
        let mut wb = Workbook::new();
        wb.sheets.clear();
        Parser {
            r1c1_to_a1,
            wb,
            report: ReadReport::default(),
            stack: Vec::new(),
            styles: HashMap::new(),
            ids: HashMap::new(),
            style: None,
            sheet: None,
            cell: None,
            option: None,
            cells: 0,
            text: 0,
            merges: 0,
            lines: 0,
        }
    }

    fn run(&mut self, bytes: &[u8]) -> Result<(), IoError> {
        let mut reader = Reader::from_reader(bytes);
        let mut root = false;
        loop {
            match reader.read_event().map_err(xml_error)? {
                Event::Start(e) => {
                    if self.stack.len() >= crate::xml::MAX_DEPTH {
                        return Err(xml_error("nested too deeply"));
                    }
                    if self.stack.is_empty() {
                        if root || local(e.name().as_ref()) != b"Workbook" {
                            return Err(xml_error("expected one Workbook root"));
                        }
                        root = true;
                    }
                    self.start(&e)?;
                    self.stack.push(local(e.name().as_ref()).to_vec());
                }
                Event::Empty(e) => {
                    if self.stack.is_empty() {
                        return Err(xml_error("expected a Workbook with worksheets"));
                    }
                    self.start(&e)?;
                    self.stack.push(local(e.name().as_ref()).to_vec());
                    self.end()?;
                }
                Event::End(_) => self.end()?,
                Event::Text(t) => {
                    let s = t.xml_content(XmlVersion::Explicit1_0).map_err(xml_error)?;
                    self.append(&s)?;
                }
                Event::CData(t) => self.append(&t.decode().map_err(xml_error)?)?,
                Event::GeneralRef(r) => {
                    let ch = match r.resolve_char_ref().map_err(xml_error)? {
                        Some(c) => c,
                        None => match r.as_ref() {
                            b"amp" => '&',
                            b"lt" => '<',
                            b"gt" => '>',
                            b"apos" => '\'',
                            b"quot" => '"',
                            _ => return Err(xml_error("unsupported XML entity")),
                        },
                    };
                    self.append(ch.encode_utf8(&mut [0; 4]))?;
                }
                Event::DocType(_) => return Err(xml_error("DOCTYPE is not supported")),
                Event::Eof => break,
                _ => {}
            }
        }
        if !root || !self.stack.is_empty() {
            return Err(xml_error("incomplete document"));
        }
        Ok(())
    }

    fn parent(&self) -> &[u8] {
        self.stack.last().map_or(&[], Vec::as_slice)
    }

    fn start(&mut self, e: &BytesStart<'_>) -> Result<(), IoError> {
        let name = e.name();
        let name = local(name.as_ref());
        let parent = self.parent().to_vec();
        match (name, parent.as_slice()) {
            (b"Style", b"Styles") => self.start_style(e),
            (b"Font" | b"Interior" | b"Alignment" | b"NumberFormat" | b"Protection" | b"Border", _) if self.stack.iter().any(|s| s == b"Styles") => {
                let a = attrs(e)?;
                if let Some((_, s)) = self.style.as_mut() {
                    apply_style(s, name, &a);
                }
                Ok(())
            }
            (b"Worksheet", _) if self.stack.len() == 1 => self.start_worksheet(e),
            (b"Column", b"Table") => self.start_column(e),
            (b"Row", b"Table") => self.start_row(e),
            (b"Cell", b"Row") => self.start_cell(e),
            (b"Data", b"Cell") => {
                let a = attrs(e)?;
                let depth = self.stack.len();
                if let Some(c) = self.cell.as_mut() {
                    c.kind = a.get("Type").cloned();
                    c.data = Some(depth);
                }
                Ok(())
            }
            (_, b"WorksheetOptions") => {
                self.start_option(name);
                Ok(())
            }
            (b"Comment", b"Cell") => {
                self.report.warn("Cell comments in the XML spreadsheet are not imported yet.");
                Ok(())
            }
            (b"NamedRange", _) => {
                self.report.warn("Named ranges in the XML spreadsheet are not imported yet.");
                Ok(())
            }
            (b"ConditionalFormatting", _) => {
                self.report.warn("Conditional formatting in the XML spreadsheet is not imported yet.");
                Ok(())
            }
            (b"DataValidation", _) => {
                self.report.warn("Data validation in the XML spreadsheet is not imported yet.");
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn start_style(&mut self, e: &BytesStart<'_>) -> Result<(), IoError> {
        let a = attrs(e)?;
        let id = a.get("ID").cloned().unwrap_or_default();
        // A style without a parent builds on Default, the workbook's Normal style.
        let parent = a.get("Parent").map_or("Default", String::as_str);
        let base = if id == "Default" { Style::default() } else { self.styles.get(parent).cloned().unwrap_or_default() };
        self.style = Some((id, base));
        Ok(())
    }

    fn start_worksheet(&mut self, e: &BytesStart<'_>) -> Result<(), IoError> {
        if self.wb.sheets.len() >= MAX_SHEETS {
            return Err(IoError::TooLarge(format!("the XML spreadsheet has more than {MAX_SHEETS} worksheets")));
        }
        let a = attrs(e)?;
        let original = a.get("Name").cloned().unwrap_or_default();
        let mut name = crate::read::SheetNames::of(&self.wb).unique(&original, self.wb.sheets.len());
        if self.wb.check_sheet_name(&name, None).is_err() {
            name = self.wb.next_sheet_name();
        }
        let mut sheet = Sheet::new(name);
        sheet.right_to_left = flag(&a, "RightToLeft");
        self.sheet = Some(SheetState { sheet, row: 0, row_span: 0, col: 0, column: 0, freeze: false, split_rows: 0, split_cols: 0 });
        Ok(())
    }

    fn start_column(&mut self, e: &BytesStart<'_>) -> Result<(), IoError> {
        let a = attrs(e)?;
        let Some(s) = self.sheet.as_mut() else { return Ok(()) };
        if let Some(i) = index(&a, "Index", MAX_COLS) {
            s.column = i;
        }
        // `ss:Span` repeats the column's settings over that many more columns.
        let span = count(&a, "Span").min(MAX_COLS);
        let info = LineInfo { size: points(&a, "Width").map(pt_to_px), hidden: flag(&a, "Hidden"), ..LineInfo::default() };
        if info != LineInfo::default() {
            for c in s.column..s.column.saturating_add(span).saturating_add(1).min(MAX_COLS) {
                if self.lines >= MAX_LINES {
                    break;
                }
                self.lines += 1;
                s.sheet.cols.insert(c, info);
            }
        }
        s.column = s.column.saturating_add(span).saturating_add(1).min(MAX_COLS);
        Ok(())
    }

    fn start_row(&mut self, e: &BytesStart<'_>) -> Result<(), IoError> {
        let a = attrs(e)?;
        let Some(s) = self.sheet.as_mut() else { return Ok(()) };
        if let Some(i) = index(&a, "Index", MAX_ROWS) {
            s.row = i;
        }
        s.col = 0;
        // `ss:Span` repeats the row's settings over that many more rows.
        s.row_span = count(&a, "Span").min(MAX_ROWS);
        let size = points(&a, "Height").map(pt_to_px);
        let info = LineInfo { size, hidden: flag(&a, "Hidden"), custom: size.is_some() && !flag(&a, "AutoFitHeight"), ..LineInfo::default() };
        if info != LineInfo::default() {
            for r in s.row..s.row.saturating_add(s.row_span).saturating_add(1).min(MAX_ROWS) {
                if self.lines >= MAX_LINES {
                    break;
                }
                self.lines += 1;
                s.sheet.rows.insert(r, info);
            }
        }
        Ok(())
    }

    fn start_cell(&mut self, e: &BytesStart<'_>) -> Result<(), IoError> {
        let a = attrs(e)?;
        let Some(s) = self.sheet.as_mut() else { return Ok(()) };
        if let Some(i) = index(&a, "Index", MAX_COLS) {
            s.col = i;
        }
        let at = CellRef::new(s.row, s.col);
        let across = count(&a, "MergeAcross");
        s.col = s.col.saturating_add(across).saturating_add(1).min(MAX_COLS);
        if at.row >= MAX_ROWS || at.col >= MAX_COLS {
            self.report.warn("Some cells outside the worksheet grid were skipped.");
            return Ok(());
        }
        if a.contains_key("ArrayRange") {
            self.report.warn("Array formulas in the XML spreadsheet were read as ordinary formulas.");
        }
        self.cell = Some(CellState {
            at,
            style: a.get("StyleID").cloned(),
            formula: a.get("Formula").cloned(),
            across,
            down: count(&a, "MergeDown"),
            kind: None,
            text: String::new(),
            data: None,
        });
        Ok(())
    }

    /// A child of `WorksheetOptions`.
    fn start_option(&mut self, name: &[u8]) {
        let Some(s) = self.sheet.as_mut() else { return };
        match name {
            b"FreezePanes" | b"FrozenNoSplit" => s.freeze = true,
            b"DisplayRightToLeft" => s.sheet.right_to_left = true,
            b"DoNotDisplayGridlines" => s.sheet.show_gridlines = false,
            b"SplitHorizontal" | b"SplitVertical" | b"Visible" => self.option = Some((name.to_vec(), String::new())),
            _ => {}
        }
    }

    fn append(&mut self, text: &str) -> Result<(), IoError> {
        if let Some((_, t)) = self.option.as_mut() {
            if t.len() < 64 {
                t.push_str(text);
            }
            return Ok(());
        }
        let Some(c) = self.cell.as_mut() else { return Ok(()) };
        // Everything inside the cell's own Data, including the HTML runs of rich text.
        if c.data.is_some_and(|d| self.stack.len() > d) {
            if c.text.len().saturating_add(text.len()) > MAX_CELL_TEXT * 4 {
                return Err(IoError::TooLarge("an XML Spreadsheet cell holds too much text".into()));
            }
            c.text.push_str(text);
        }
        Ok(())
    }

    fn end(&mut self) -> Result<(), IoError> {
        let name = self.stack.pop().ok_or_else(|| xml_error("unexpected closing element"))?;
        let depth = self.stack.len();
        match name.as_slice() {
            b"Style" if self.parent() == b"Styles" => {
                if let Some((id, style)) = self.style.take()
                    && !id.is_empty()
                    && self.styles.len() < MAX_STYLES
                {
                    self.styles.insert(id, style);
                }
            }
            b"Data" => {
                if let Some(c) = self.cell.as_mut()
                    && c.data == Some(depth)
                {
                    c.data = None;
                }
            }
            b"Cell" if self.parent() == b"Row" => self.finish_cell()?,
            b"Row" if self.parent() == b"Table" => {
                if let Some(s) = self.sheet.as_mut() {
                    s.row = s.row.saturating_add(s.row_span).saturating_add(1).min(MAX_ROWS);
                    s.row_span = 0;
                }
            }
            b"SplitHorizontal" | b"SplitVertical" | b"Visible" => {
                if let Some((option, text)) = self.option.take()
                    && let Some(s) = self.sheet.as_mut()
                {
                    let text = text.trim();
                    match option.as_slice() {
                        b"SplitHorizontal" => s.split_rows = text.parse::<u32>().unwrap_or(0).min(MAX_ROWS - 1),
                        b"SplitVertical" => s.split_cols = text.parse::<u32>().unwrap_or(0).min(MAX_COLS - 1),
                        _ => {
                            s.sheet.visibility = match text {
                                "SheetHidden" => Visibility::Hidden,
                                "SheetVeryHidden" => Visibility::VeryHidden,
                                _ => Visibility::Visible,
                            }
                        }
                    }
                }
            }
            b"Worksheet" if depth == 1 => {
                if let Some(mut s) = self.sheet.take() {
                    if s.freeze && (s.split_rows > 0 || s.split_cols > 0) {
                        s.sheet.freeze = Some((s.split_rows, s.split_cols));
                    }
                    self.wb.sheets.push(Arc::new(s.sheet));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn style_id(&mut self, id: Option<&str>) -> StyleId {
        let id = id.unwrap_or("Default");
        if let Some(s) = self.ids.get(id) {
            return *s;
        }
        let style = match self.styles.get(id) {
            Some(s) => self.wb.styles.intern(s.clone()),
            None => StyleId::DEFAULT,
        };
        self.ids.insert(id.to_string(), style);
        style
    }

    fn finish_cell(&mut self) -> Result<(), IoError> {
        let Some(c) = self.cell.take() else { return Ok(()) };
        let value = match c.kind.as_deref() {
            None => Value::Empty,
            Some("Number") => match c.text.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => Value::Number(n),
                _ => Value::Text(Arc::from(c.text.as_str())),
            },
            Some("Boolean") => Value::Bool(matches!(c.text.trim(), "1" | "true" | "TRUE" | "True")),
            Some("DateTime") => date_time(c.text.trim()).map_or_else(|| Value::Text(Arc::from(c.text.as_str())), Value::Number),
            Some("Error") => CellError::parse(c.text.trim()).map_or_else(|| Value::Text(Arc::from(c.text.as_str())), Value::Error),
            Some(_) => Value::Text(Arc::from(c.text.as_str())),
        };
        let style = self.style_id(c.style.as_deref());
        let mut cell = Cell { style, ..Cell::value(value) };
        if let Some(f) = c.formula.as_deref() {
            let body = f.trim().strip_prefix('=').unwrap_or(f.trim());
            let formula = Formula::new(&(self.r1c1_to_a1)(body, c.at));
            if formula.expr.is_some() {
                cell.formula = Some(Arc::new(formula));
            } else {
                self.report.warn("Some formulas in the XML spreadsheet could not be read; their saved results were kept.");
            }
        }
        if let Value::Text(t) = &cell.value {
            self.text = self.text.saturating_add(t.len() as u64);
        }
        let Some(s) = self.sheet.as_mut() else { return Ok(()) };
        if c.across > 0 || c.down > 0 {
            let last = CellRef::new(c.at.row.saturating_add(c.down).min(MAX_ROWS - 1), c.at.col.saturating_add(c.across).min(MAX_COLS - 1));
            self.merges += 1;
            if self.merges > MAX_MERGES {
                return Err(IoError::TooLarge(format!("the XML spreadsheet has more than {MAX_MERGES} merged ranges")));
            }
            s.sheet.merges.push(RangeRef::new(c.at, last));
        }
        if cell.is_blank() && cell.style == StyleId::DEFAULT {
            return Ok(());
        }
        self.cells += 1;
        if self.cells > MAX_CELLS || self.text > MAX_TEXT {
            return Err(IoError::TooLarge("the XML spreadsheet expands beyond the import limit (5 million cells or 256 MB of text)".into()));
        }
        s.sheet.set_cell(c.at, cell);
        Ok(())
    }
}

/// A `DateTime` value (`2024-01-15T13:30:00.000`) as a serial number. Times alone are written
/// on Excel's day zero, `1899-12-31`.
fn date_time(s: &str) -> Option<f64> {
    if let Some(time) = s.strip_prefix("1899-12-31T") {
        let (serial, _) = crate::ods::parse_date(&format!("1900-01-01T{time}"))?;
        return Some(serial - 1.0);
    }
    crate::ods::parse_date(s).map(|(serial, _)| serial)
}

/// Applies one child element of a `Style` to the style.
fn apply_style(s: &mut Style, element: &[u8], a: &HashMap<String, String>) {
    let color = |key: &str| a.get(key).and_then(|c| Color::from_hex(c));
    match element {
        b"Font" => {
            if let Some(n) = a.get("FontName").filter(|n| !n.trim().is_empty()) {
                s.font.name = n.trim().to_string();
            }
            if let Some(size) = a.get("Size").and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| (1.0..=409.0).contains(v)) {
                s.font.size = size;
            }
            if a.contains_key("Bold") {
                s.font.bold = flag(a, "Bold");
            }
            if a.contains_key("Italic") {
                s.font.italic = flag(a, "Italic");
            }
            if a.contains_key("StrikeThrough") {
                s.font.strike = flag(a, "StrikeThrough");
            }
            if let Some(u) = a.get("Underline") {
                s.font.underline = match u.as_str() {
                    "Single" => Underline::Single,
                    "Double" => Underline::Double,
                    "SingleAccounting" => Underline::SingleAccounting,
                    "DoubleAccounting" => Underline::DoubleAccounting,
                    _ => Underline::None,
                };
            }
            if let Some(v) = a.get("VerticalAlign") {
                s.font.vert = match v.as_str() {
                    "Superscript" => VertAlign::Superscript,
                    "Subscript" => VertAlign::Subscript,
                    _ => VertAlign::Baseline,
                };
            }
            if let Some(c) = color("Color") {
                s.font.color = c;
            }
        }
        b"Interior" => {
            let pattern = match a.get("Pattern").map(String::as_str) {
                None | Some("None") => PatternType::None,
                Some("Solid") => PatternType::Solid,
                Some("Gray75") => PatternType::Gray75,
                Some("Gray50") => PatternType::Gray50,
                Some("Gray25") => PatternType::Gray25,
                Some("Gray125") => PatternType::Gray125,
                Some("Gray0625") => PatternType::Gray0625,
                Some("HorzStripe") => PatternType::DarkHorizontal,
                Some("VertStripe") => PatternType::DarkVertical,
                Some("ReverseDiagStripe") => PatternType::DarkUp,
                Some("DiagStripe") => PatternType::DarkDown,
                Some("DiagCross") => PatternType::DarkGrid,
                Some("ThickDiagCross") => PatternType::DarkTrellis,
                Some("ThinHorzStripe") => PatternType::LightHorizontal,
                Some("ThinVertStripe") => PatternType::LightVertical,
                Some("ThinReverseDiagStripe") => PatternType::LightUp,
                Some("ThinDiagStripe") => PatternType::LightDown,
                Some("ThinHorzCross") => PatternType::LightGrid,
                Some("ThinDiagCross") => PatternType::LightTrellis,
                Some(_) => PatternType::Solid,
            };
            match (pattern, color("Color")) {
                (PatternType::None, _) => s.fill = Fill::default(),
                (PatternType::Solid, c) => s.fill = Fill::solid(c.unwrap_or(Color::Auto)),
                (p, c) => s.fill = Fill { pattern: p, fg: color("PatternColor").unwrap_or(Color::Auto), bg: c.unwrap_or(Color::Auto) },
            }
        }
        b"Alignment" => {
            if let Some(h) = a.get("Horizontal") {
                s.align.h = match h.as_str() {
                    "Left" => HAlign::Left,
                    "Center" => HAlign::Center,
                    "Right" => HAlign::Right,
                    "Fill" => HAlign::Fill,
                    "Justify" => HAlign::Justify,
                    "CenterAcrossSelection" => HAlign::CenterAcross,
                    "Distributed" | "JustifyDistributed" => HAlign::Distributed,
                    _ => HAlign::General,
                };
            }
            if let Some(v) = a.get("Vertical") {
                s.align.v = match v.as_str() {
                    "Top" => VAlign::Top,
                    "Center" => VAlign::Center,
                    "Justify" => VAlign::Justify,
                    "Distributed" | "JustifyDistributed" => VAlign::Distributed,
                    _ => VAlign::Bottom,
                };
            }
            if a.contains_key("WrapText") {
                s.align.wrap = flag(a, "WrapText");
            }
            if a.contains_key("ShrinkToFit") {
                s.align.shrink = flag(a, "ShrinkToFit");
            }
            if let Some(i) = a.get("Indent").and_then(|v| v.trim().parse::<u8>().ok()) {
                s.align.indent = i.min(250);
            }
            if let Some(r) = a.get("Rotate").and_then(|v| v.trim().parse::<i16>().ok()).filter(|r| (-90..=90).contains(r)) {
                s.align.rotation = r;
            }
            if flag(a, "VerticalText") {
                s.align.rotation = 255;
            }
            if let Some(o) = a.get("ReadingOrder") {
                s.align.reading_order = match o.as_str() {
                    "RightToLeft" => Some(ReadingOrder::RightToLeft),
                    "LeftToRight" => Some(ReadingOrder::LeftToRight),
                    _ => None,
                };
            }
        }
        b"NumberFormat" => {
            if let Some(f) = a.get("Format") {
                s.num_fmt = NumFmt::new(named_format(f));
            }
        }
        b"Protection" => {
            if a.contains_key("Protected") {
                s.protection.locked = flag(a, "Protected");
            }
            if a.contains_key("HideFormula") {
                s.protection.hidden = flag(a, "HideFormula");
            }
        }
        b"Border" => {
            let weight = a.get("Weight").and_then(|w| w.trim().parse::<u8>().ok()).unwrap_or(1);
            let style = match (a.get("LineStyle").map(String::as_str), weight) {
                (None | Some("None"), _) => BorderStyle::None,
                (Some("Continuous"), 0) => BorderStyle::Hair,
                (Some("Continuous"), 1) => BorderStyle::Thin,
                (Some("Continuous"), 2) => BorderStyle::Medium,
                (Some("Continuous"), _) => BorderStyle::Thick,
                (Some("Dash"), 0 | 1) => BorderStyle::Dashed,
                (Some("Dash"), _) => BorderStyle::MediumDashed,
                (Some("Dot"), _) => BorderStyle::Dotted,
                (Some("DashDot"), 0 | 1) => BorderStyle::DashDot,
                (Some("DashDot"), _) => BorderStyle::MediumDashDot,
                (Some("DashDotDot"), 0 | 1) => BorderStyle::DashDotDot,
                (Some("DashDotDot"), _) => BorderStyle::MediumDashDotDot,
                (Some("SlantDashDot"), _) => BorderStyle::SlantDashDot,
                (Some("Double"), _) => BorderStyle::Double,
                (Some(_), _) => BorderStyle::Thin,
            };
            let line = BorderLine { style, color: color("Color").unwrap_or(Color::Auto) };
            match a.get("Position").map(String::as_str) {
                Some("Left") => s.borders.left = line,
                Some("Right") => s.borders.right = line,
                Some("Top") => s.borders.top = line,
                Some("Bottom") => s.borders.bottom = line,
                Some("DiagonalLeft") => s.borders.diag_down = line,
                Some("DiagonalRight") => s.borders.diag_up = line,
                _ => {}
            }
        }
        _ => {}
    }
}

/// The format code a `NumberFormat` names: Excel writes its built-in formats by name.
fn named_format(f: &str) -> &str {
    match f {
        "General" | "General Number" => "General",
        "General Date" => "m/d/yyyy h:mm",
        "Long Date" => "dddd, mmmm d, yyyy",
        "Medium Date" => "d-mmm-yy",
        "Short Date" => "m/d/yyyy",
        "Long Time" => "h:mm:ss AM/PM",
        "Medium Time" => "h:mm AM/PM",
        "Short Time" => "h:mm",
        "Currency" => "$#,##0.00_);[Red]($#,##0.00)",
        "Euro Currency" => "[$€-2] #,##0.00",
        "Fixed" => "0.00",
        "Standard" => "#,##0.00",
        "Percent" => "0.00%",
        "Scientific" => "0.00E+00",
        "Yes/No" => "\"Yes\";\"Yes\";\"No\"",
        "True/False" => "\"True\";\"True\";\"False\"",
        "On/Off" => "\"On\";\"On\";\"Off\"",
        other => other,
    }
}
