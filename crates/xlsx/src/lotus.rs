//! Data-only import of Lotus 1-2-3 Release 1–2 worksheets (`.wks`, `.wk1`), Symphony worksheets
//! (`.wrk`, `.wr1`) and Quattro Pro for DOS notebooks (`.wq1`, `.wq2`), which share one record
//! layout. Clean-room from the published Lotus *Worksheet File Format* description; all test files
//! are built in Rust.
//!
//! A file is a run of records: a little-endian `u16` type, a `u16` body length and the body. It
//! starts with BOF (type 0, a `u16` version) and ends with EOF (type 1). Cell records start with a
//! format byte, then a `u16` column and a `u16` row (0-based):
//!
//! | type | record  | after the cell address |
//! |------|---------|------------------------|
//! | 0x0C | BLANK   | (formatted empty cell) |
//! | 0x0D | INTEGER | `i16` |
//! | 0x0E | NUMBER  | IEEE `f64` |
//! | 0x0F | LABEL   | NUL-terminated text, first character the alignment prefix (`'` `"` `^` `\` `|`) |
//! | 0x10 | FORMULA | `f64` saved result, `u16` size, compiled formula |
//! | 0x33 | STRING  | the text result of the formula just before it (NUL-terminated) |
//!
//! Column widths come from COLW1 (0x08: a `u8` column and a `u8` width in characters). The format
//! byte holds protection (bit 7), the format type (bits 4–6) and decimal places or, for the
//! special type 7, which special format (bits 0–3).
//!
//! Formulas are not translated: like the ODS import, a formula cell keeps its saved result as a
//! constant. Lotus serial dates count days the way Excel's 1900 date system does, so date cells
//! read as the same dates. Release 3 and later (`.wk3`, `.wk4`, `.123`) and Quattro Pro for
//! Windows (`.wb1`–`.wb3`, `.qpw`) use different, multi-sheet layouts and are refused with a
//! message saying so.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, LineInfo, NumFmt, Sheet, Style, StyleId, Workbook};

use crate::tables::col_width_to_px;
use crate::{IoError, ReadReport};

/// BOF versions of the formats read here.
const LOTUS_VERSIONS: [u16; 3] = [0x0404, 0x0405, 0x0406];
/// Quattro Pro for DOS (WQ1, WQ2) writes the same layout under its own versions.
const QUATTRO_VERSIONS: [u16; 2] = [0x5120, 0x5121];
/// The largest file read; real worksheets of this generation are far smaller.
const MAX_BYTES: usize = 256 << 20;
/// Cells kept from one file (a Lotus sheet holds at most 256 × 8192).
const MAX_CELLS: u64 = 4_000_000;
/// Text kept from one file.
const MAX_TEXT: u64 = 128 << 20;

const BOF: u16 = 0x0000;
const EOF: u16 = 0x0001;
const COLW1: u16 = 0x0008;
const BLANK: u16 = 0x000C;
const INTEGER: u16 = 0x000D;
const NUMBER: u16 = 0x000E;
const LABEL: u16 = 0x000F;
const FORMULA: u16 = 0x0010;
const STRING: u16 = 0x0033;

/// Which product wrote a file, from its first record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flavor {
    Lotus,
    Quattro,
    /// Lotus 1-2-3 Release 3 and later, or Quattro Pro for Windows: recognised, not read.
    Later,
}

/// Recognises the BOF record a Lotus-family worksheet starts with.
pub(crate) fn flavor(bytes: &[u8]) -> Option<Flavor> {
    let head = bytes.get(..6)?;
    if head.get(..2)? != [0, 0] {
        return None;
    }
    let len = u16::from_le_bytes([head[2], head[3]]);
    let version = u16::from_le_bytes([head[4], head[5]]);
    if len == 2 && LOTUS_VERSIONS.contains(&version) {
        Some(Flavor::Lotus)
    } else if len == 2 && QUATTRO_VERSIONS.contains(&version) {
        Some(Flavor::Quattro)
    } else if (0x1000..=0x1005).contains(&version) && (2..=64).contains(&len) {
        Some(Flavor::Later)
    } else {
        None
    }
}

/// Reads a Lotus 1-2-3 (`.wks`, `.wk1`), Symphony (`.wrk`, `.wr1`) or Quattro Pro for DOS
/// (`.wq1`, `.wq2`) worksheet into a one-sheet workbook. Formula cells keep their saved results;
/// always inspect the returned warnings.
pub fn read_lotus(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    if bytes.len() > MAX_BYTES {
        return Err(IoError::TooLarge(format!("the worksheet is larger than {} MB", MAX_BYTES >> 20)));
    }
    let product = match flavor(bytes) {
        Some(Flavor::Lotus) => "Lotus 1-2-3",
        Some(Flavor::Quattro) => "Quattro Pro",
        Some(Flavor::Later) => {
            return Err(IoError::Format(
                "this is a Lotus 1-2-3 Release 3 or later (.wk3, .wk4, .123) or Quattro Pro for Windows file. GridCraft reads \
                 Lotus 1-2-3 Release 1–2 (.wks, .wk1) and Quattro Pro for DOS (.wq1, .wq2) worksheets; save it in one of those \
                 formats, or as .xlsx, and try again."
                    .into(),
            ));
        }
        // Quattro Pro for Windows 7 and later (.qpw, .wb3) keep the notebook in a compound file.
        None if bytes.starts_with(&crate::cfb::SIGNATURE) => {
            return Err(IoError::Format(
                "this is a Quattro Pro for Windows notebook (.qpw, .wb3), which GridCraft can't read. Save it from Quattro Pro as \
                 .xlsx, or as a Quattro Pro for DOS (.wq1) or Lotus 1-2-3 (.wk1) worksheet, and try again."
                    .into(),
            ));
        }
        None => return Err(IoError::Format("not a Lotus 1-2-3 or Quattro Pro worksheet (no BOF record)".into())),
    };
    let mut r = Reader { wb: Workbook::new(), sheet: Sheet::new("Sheet1"), report: ReadReport::default(), styles: Vec::new(), cells: 0, text: 0 };
    r.report.warn(format!(
        "{product} data-only import: formulas become their saved results and will not recalculate. Fonts, colours, named ranges, \
         graphs and print settings are not imported. Save a new .xlsx file; {product} saving is not supported."
    ));
    let mut pos = 0usize;
    // The cell the last FORMULA record was in: a STRING record right after it holds its text result.
    let mut last_formula: Option<CellRef> = None;
    let mut ended = false;
    while let Some(header) = bytes.get(pos..pos.saturating_add(4)) {
        let kind = u16::from_le_bytes([header[0], header[1]]);
        let len = usize::from(u16::from_le_bytes([header[2], header[3]]));
        let start = pos + 4;
        let Some(body) = bytes.get(start..start.saturating_add(len)) else {
            r.report.warn("The file ends in the middle of a record; the cells before it were imported.");
            break;
        };
        pos = start + len;
        let formula_before = last_formula.take();
        match kind {
            BOF => {}
            EOF => {
                ended = true;
                break;
            }
            COLW1 => {
                if let [col, width, ..] = *body {
                    let px = col_width_to_px(f64::from(width));
                    if px > 0.0 {
                        r.sheet.cols.insert(u32::from(col), LineInfo { size: Some(px), ..LineInfo::default() });
                    }
                }
            }
            BLANK => {
                // A formatted empty cell carries nothing to show.
            }
            INTEGER => {
                if let Some((format, at, rest)) = r.address(body)
                    && let Some(n) = rest.get(..2)
                {
                    r.put(at, Value::Number(f64::from(i16::from_le_bytes([n[0], n[1]]))), format)?;
                }
            }
            NUMBER => {
                if let Some((format, at, rest)) = r.address(body) {
                    let value = number(rest);
                    r.put(at, value, format)?;
                }
            }
            LABEL => {
                if let Some((_, at, rest)) = r.address(body) {
                    let text = label(rest);
                    r.put(at, Value::Text(Arc::from(text)), None)?;
                }
            }
            FORMULA => {
                if let Some((format, at, rest)) = r.address(body) {
                    let value = number(rest);
                    r.put(at, value, format)?;
                    last_formula = Some(at);
                }
            }
            STRING => {
                // The text result of a string formula replaces the number its FORMULA record held.
                if let Some((_, at, rest)) = r.address(body)
                    && formula_before == Some(at)
                {
                    let text = text(rest);
                    r.put(at, Value::Text(Arc::from(text)), None)?;
                }
            }
            _ => {}
        }
    }
    if !ended {
        r.report.warn("The file has no end-of-file record; it may be truncated. The cells it holds were imported.");
    }
    let Reader { mut wb, mut sheet, report, .. } = r;
    if let Some(first) = wb.sheet_mut(0) {
        sheet.name = std::mem::take(&mut first.name);
        *first = sheet;
    }
    Ok((wb, report))
}

struct Reader {
    wb: Workbook,
    sheet: Sheet,
    report: ReadReport,
    /// Number formats already made into styles.
    styles: Vec<(&'static str, StyleId)>,
    cells: u64,
    text: u64,
}

impl Reader {
    /// A cell record's format byte, cell and the rest of its body; `None` (with a warning) for a
    /// short record or a cell outside the grid.
    fn address<'a>(&mut self, body: &'a [u8]) -> Option<(Option<&'static str>, CellRef, &'a [u8])> {
        let Some(&[format, c0, c1, r0, r1]) = body.get(..5).and_then(|h| <&[u8; 5]>::try_from(h).ok()) else {
            self.report.warn("Some damaged cell records were skipped.");
            return None;
        };
        let (col, row) = (u32::from(u16::from_le_bytes([c0, c1])), u32::from(u16::from_le_bytes([r0, r1])));
        if col >= MAX_COLS || row >= MAX_ROWS {
            self.report.warn("Some cells outside the worksheet grid were skipped.");
            return None;
        }
        Some((number_format(format), CellRef::new(row, col), body.get(5..).unwrap_or_default()))
    }

    fn put(&mut self, at: CellRef, value: Value, format: Option<&'static str>) -> Result<(), IoError> {
        if self.sheet.cell(at).is_none() {
            self.cells += 1;
            if self.cells > MAX_CELLS {
                return Err(IoError::TooLarge(format!("the worksheet holds more than {MAX_CELLS} cells")));
            }
        }
        if let Value::Text(t) = &value {
            self.text = self.text.saturating_add(t.len() as u64);
            if self.text > MAX_TEXT {
                return Err(IoError::TooLarge(format!("the worksheet holds more than {} MB of text", MAX_TEXT >> 20)));
            }
        }
        let style = match format {
            Some(code) => match self.styles.iter().find(|(f, _)| *f == code) {
                Some((_, id)) => *id,
                None => {
                    let id = self.wb.styles.intern(Style { num_fmt: NumFmt::new(code), ..Style::default() });
                    self.styles.push((code, id));
                    id
                }
            },
            None => StyleId::DEFAULT,
        };
        self.sheet.set_cell(at, Cell { style, ..Cell::value(value) });
        Ok(())
    }
}

/// An `f64` cell value. Lotus marks `ERR` and `NA` results with NaN bit patterns; both read as
/// an error, since a sheet can't hold a NaN.
fn number(rest: &[u8]) -> Value {
    match rest.get(..8).and_then(|b| <[u8; 8]>::try_from(b).ok()) {
        Some(b) => {
            let n = f64::from_le_bytes(b);
            if n.is_finite() { Value::Number(n) } else { Value::Error(CellError::NA) }
        }
        None => Value::Error(CellError::Value),
    }
}

/// A label's text without its alignment prefix.
fn label(rest: &[u8]) -> String {
    let t = text(rest);
    match t.chars().next() {
        Some('\'' | '"' | '^' | '\\' | '|') => t.chars().skip(1).collect(),
        _ => t,
    }
}

/// NUL-terminated text. Characters past ASCII are read as Latin-1 (the Lotus International
/// Character Set agrees with it for the common accented letters).
fn text(rest: &[u8]) -> String {
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    rest.iter().take(end).map(|&b| char::from(b)).collect()
}

/// The number format a cell's format byte names; `None` for General (and the sheet default).
pub(crate) fn number_format(byte: u8) -> Option<&'static str> {
    const FIXED: [&str; 16] = [
        "0",
        "0.0",
        "0.00",
        "0.000",
        "0.0000",
        "0.00000",
        "0.000000",
        "0.0000000",
        "0.00000000",
        "0.000000000",
        "0.0000000000",
        "0.00000000000",
        "0.000000000000",
        "0.0000000000000",
        "0.00000000000000",
        "0.000000000000000",
    ];
    const SCI: [&str; 16] = [
        "0E+00",
        "0.0E+00",
        "0.00E+00",
        "0.000E+00",
        "0.0000E+00",
        "0.00000E+00",
        "0.000000E+00",
        "0.0000000E+00",
        "0.00000000E+00",
        "0.000000000E+00",
        "0.0000000000E+00",
        "0.00000000000E+00",
        "0.000000000000E+00",
        "0.0000000000000E+00",
        "0.00000000000000E+00",
        "0.000000000000000E+00",
    ];
    const CURRENCY: [&str; 16] = [
        "$#,##0_);($#,##0)",
        "$#,##0.0_);($#,##0.0)",
        "$#,##0.00_);($#,##0.00)",
        "$#,##0.000_);($#,##0.000)",
        "$#,##0.0000_);($#,##0.0000)",
        "$#,##0.00000_);($#,##0.00000)",
        "$#,##0.000000_);($#,##0.000000)",
        "$#,##0.0000000_);($#,##0.0000000)",
        "$#,##0.00000000_);($#,##0.00000000)",
        "$#,##0.000000000_);($#,##0.000000000)",
        "$#,##0.0000000000_);($#,##0.0000000000)",
        "$#,##0.00000000000_);($#,##0.00000000000)",
        "$#,##0.000000000000_);($#,##0.000000000000)",
        "$#,##0.0000000000000_);($#,##0.0000000000000)",
        "$#,##0.00000000000000_);($#,##0.00000000000000)",
        "$#,##0.000000000000000_);($#,##0.000000000000000)",
    ];
    const PERCENT: [&str; 16] = [
        "0%",
        "0.0%",
        "0.00%",
        "0.000%",
        "0.0000%",
        "0.00000%",
        "0.000000%",
        "0.0000000%",
        "0.00000000%",
        "0.000000000%",
        "0.0000000000%",
        "0.00000000000%",
        "0.000000000000%",
        "0.0000000000000%",
        "0.00000000000000%",
        "0.000000000000000%",
    ];
    const COMMA: [&str; 16] = [
        "#,##0_);(#,##0)",
        "#,##0.0_);(#,##0.0)",
        "#,##0.00_);(#,##0.00)",
        "#,##0.000_);(#,##0.000)",
        "#,##0.0000_);(#,##0.0000)",
        "#,##0.00000_);(#,##0.00000)",
        "#,##0.000000_);(#,##0.000000)",
        "#,##0.0000000_);(#,##0.0000000)",
        "#,##0.00000000_);(#,##0.00000000)",
        "#,##0.000000000_);(#,##0.000000000)",
        "#,##0.0000000000_);(#,##0.0000000000)",
        "#,##0.00000000000_);(#,##0.00000000000)",
        "#,##0.000000000000_);(#,##0.000000000000)",
        "#,##0.0000000000000_);(#,##0.0000000000000)",
        "#,##0.00000000000000_);(#,##0.00000000000000)",
        "#,##0.000000000000000_);(#,##0.000000000000000)",
    ];
    let detail = usize::from(byte & 0x0F);
    let pick = |table: &[&'static str; 16]| table.get(detail).copied();
    match (byte >> 4) & 0x07 {
        0 => pick(&FIXED),
        1 => pick(&SCI),
        2 => pick(&CURRENCY),
        3 => pick(&PERCENT),
        4 => pick(&COMMA),
        7 => match detail {
            2 => Some("d-mmm-yy"),
            3 => Some("d-mmm"),
            4 => Some("mmm-yy"),
            5 => Some("@"),
            6 => Some(";;;"),
            7 => Some("h:mm:ss AM/PM"),
            8 => Some("h:mm AM/PM"),
            9 => Some("mm/dd/yy"),
            10 => Some("mm/dd"),
            11 => Some("hh:mm:ss"),
            12 => Some("hh:mm"),
            // +/- bar graph, General, and the sheet default.
            _ => None,
        },
        _ => None,
    }
}
