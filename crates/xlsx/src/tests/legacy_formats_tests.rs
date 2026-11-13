//! Lotus 1-2-3 / Quattro Pro worksheets, OpenOffice.org 1.x (`.sxc`) packages and XML Spreadsheet
//! 2003 documents, all assembled in Rust from the published layouts (no files from another
//! application).

use gridcraft_core::{CellError, CellRef, RangeRef, Value};
use gridcraft_model::{Color, HAlign, PatternType, Visibility, Workbook};

use super::make_zip;
use crate::{Format, read_lotus, read_ods, read_xml2003, sniff};

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}
fn text(s: &str) -> Value {
    Value::Text(s.into())
}
fn fmt(wb: &Workbook, a: &str) -> String {
    let sh = wb.sheet(0).unwrap();
    wb.styles.get(sh.style_id(at(a))).num_fmt.0.to_string()
}

// ---------------------------------------------------------------- Lotus 1-2-3 / Quattro Pro

fn record(kind: u16, body: &[u8]) -> Vec<u8> {
    let mut out = kind.to_le_bytes().to_vec();
    out.extend((body.len() as u16).to_le_bytes());
    out.extend(body);
    out
}
fn cell(kind: u16, format: u8, col: u16, row: u16, rest: &[u8]) -> Vec<u8> {
    let mut body = vec![format];
    body.extend(col.to_le_bytes());
    body.extend(row.to_le_bytes());
    body.extend(rest);
    record(kind, &body)
}
fn worksheet(version: u16, records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = record(0x0000, &version.to_le_bytes());
    for r in records {
        out.extend(r);
    }
    out.extend(record(0x0001, &[]));
    out
}
/// A formula record: saved result, then a compiled formula (a constant and the end marker).
fn formula(format: u8, col: u16, row: u16, result: f64) -> Vec<u8> {
    let mut rest = result.to_le_bytes().to_vec();
    let code = [0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0x03];
    rest.extend((code.len() as u16).to_le_bytes());
    rest.extend(code);
    cell(0x0010, format, col, row, &rest)
}

fn sample_wk1(version: u16) -> Vec<u8> {
    worksheet(
        version,
        &[
            record(0x0008, &[0, 20]),
            cell(0x000F, 0xFF, 0, 0, b"'Item\0"),
            cell(0x000F, 0xFF, 1, 0, b"^Total\0"),
            cell(0x000F, 0xFF, 0, 1, b"\"Caf\xe9\0"),
            cell(0x000D, 0xFF, 1, 1, &(-42i16).to_le_bytes()),
            // Fixed with 2 decimals; currency with 0; percent with 1; date (special 2).
            cell(0x000E, 0x02, 2, 1, &1234.5f64.to_le_bytes()),
            cell(0x000E, 0x20, 3, 1, &99.0f64.to_le_bytes()),
            cell(0x000E, 0x31, 4, 1, &0.25f64.to_le_bytes()),
            cell(0x000E, 0xF2, 5, 1, &45_000.0f64.to_le_bytes()),
            formula(0xFF, 1, 2, 7.0),
            // A string formula: the STRING record after it holds its text.
            formula(0xFF, 2, 2, 0.0),
            cell(0x0033, 0xFF, 2, 2, b"done\0"),
            // A blank formatted cell and a NaN (ERR) result.
            cell(0x000C, 0x02, 3, 2, &[]),
            cell(0x000E, 0xFF, 4, 2, &f64::NAN.to_le_bytes()),
        ],
    )
}

#[test]
fn lotus_wk1_values_labels_formats_and_formula_results_are_read() {
    let bytes = sample_wk1(0x0406);
    assert_eq!(sniff(&bytes), Format::Lotus);
    let (wb, report) = read_lotus(&bytes).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("Lotus 1-2-3 data-only import")), "{:?}", report.warnings);
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.value(at("A1")), text("Item"), "the alignment prefix is not part of the text");
    assert_eq!(sh.value(at("B1")), text("Total"));
    assert_eq!(sh.value(at("A2")), text("Café"), "bytes past ASCII read as Latin-1");
    assert_eq!(sh.value(at("B2")), Value::Number(-42.0));
    assert_eq!(sh.value(at("C2")), Value::Number(1234.5));
    assert_eq!(fmt(&wb, "C2"), "0.00");
    assert_eq!(fmt(&wb, "D2"), "$#,##0_);($#,##0)");
    assert_eq!(fmt(&wb, "E2"), "0.0%");
    assert_eq!(fmt(&wb, "F2"), "d-mmm-yy", "a protected cell (bit 7) keeps its format");
    assert_eq!(fmt(&wb, "B2"), "General", "0xFF is the sheet default");
    assert_eq!(sh.value(at("B3")), Value::Number(7.0), "a formula keeps its saved result");
    assert_eq!(sh.value(at("C3")), text("done"), "a string formula's STRING record replaces its number");
    assert!(sh.cell(at("B3")).is_some_and(|c| c.formula.is_none()), "formulas are not translated");
    assert_eq!(sh.value(at("D3")), Value::Empty);
    assert_eq!(sh.value(at("E3")), Value::Error(CellError::NA), "a NaN result is an error, never a NaN number");
    assert!(sh.cols.get(&0).and_then(|i| i.size).is_some_and(|w| w > 100.0), "COLW1 sets column A's width");
}

#[test]
fn quattro_pro_and_symphony_use_the_same_reader_and_later_versions_are_refused() {
    for version in [0x0404, 0x0405, 0x5120, 0x5121] {
        let bytes = sample_wk1(version);
        assert_eq!(sniff(&bytes), Format::Lotus, "{version:#x}");
        let (wb, _) = read_lotus(&bytes).unwrap();
        assert_eq!(wb.sheet(0).unwrap().value(at("B2")), Value::Number(-42.0), "{version:#x}");
    }
    let quattro = read_lotus(&sample_wk1(0x5120)).unwrap().1;
    assert!(quattro.warnings.iter().any(|w| w.contains("Quattro Pro")), "{:?}", quattro.warnings);
    // Release 3+ (WK3/WK4/123) and Quattro Pro for Windows: recognised, refused with a reason.
    let mut wk3 = record(0x0000, &[0x00, 0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    wk3.extend(record(0x0001, &[]));
    assert_eq!(sniff(&wk3), Format::Lotus);
    let err = read_lotus(&wk3).unwrap_err().to_string();
    assert!(err.contains("Release 3 or later") && err.contains(".wk1"), "{err}");
    // An unknown version is not taken for a worksheet.
    assert_ne!(sniff(&worksheet(0x0999, &[])), Format::Lotus);
    assert!(read_lotus(b"not a worksheet").is_err());
    let qpw = read_lotus(&crate::cfb::tests::build(&["NativeContent_MAIN"])).unwrap_err().to_string();
    assert!(qpw.contains("Quattro Pro for Windows"), "{qpw}");
}

#[test]
fn lotus_damage_is_survived() {
    // Truncated in the middle of a record: the cells before it are kept.
    let mut bytes = sample_wk1(0x0406);
    bytes.truncate(bytes.len() - 10);
    let (wb, report) = read_lotus(&bytes).unwrap();
    assert_eq!(wb.sheet(0).unwrap().value(at("A1")), text("Item"));
    assert!(report.warnings.iter().any(|w| w.contains("truncated") || w.contains("middle of a record")), "{:?}", report.warnings);
    // Short cell records, cells outside the grid and empty bodies are skipped, never a panic.
    let bytes = worksheet(
        0x0406,
        &[
            record(0x000E, &[0xFF, 1]),
            record(0x000D, &[0xFF, 0, 0, 0, 0]),
            cell(0x000E, 0xFF, u16::MAX, 0, &1.0f64.to_le_bytes()),
            record(0x000F, &[]),
            record(0x0033, &[0xFF, 0, 0, 0, 0, b'x']),
            record(0x0008, &[3]),
        ],
    );
    let (wb, report) = read_lotus(&bytes).unwrap();
    assert!(wb.sheet(0).unwrap().cells.is_empty());
    assert!(report.warnings.iter().any(|w| w.contains("outside the worksheet grid")), "{:?}", report.warnings);
    // Every prefix of a valid file reads or fails cleanly.
    let full = sample_wk1(0x0406);
    for n in 0..full.len() {
        let _ = read_lotus(&full[..n]);
    }
}

// ---------------------------------------------------------------- OpenOffice.org 1.x (.sxc)

const SXC_MANIFEST: &[u8] = br#"<manifest:manifest xmlns:manifest="http://openoffice.org/2001/manifest"><manifest:file-entry manifest:media-type="application/vnd.sun.xml.calc" manifest:full-path="/"/><manifest:file-entry manifest:media-type="text/xml" manifest:full-path="content.xml"/></manifest:manifest>"#;

fn sxc(mime: &[u8], tables: &str) -> Vec<u8> {
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="http://openoffice.org/2000/office" xmlns:table="http://openoffice.org/2000/table" xmlns:text="http://openoffice.org/2000/text" office:class="spreadsheet" office:version="1.0"><office:body>{tables}</office:body></office:document-content>"#
    );
    make_zip(&[("mimetype", mime), ("META-INF/manifest.xml", SXC_MANIFEST), ("content.xml", content.as_bytes())])
}

#[test]
fn sxc_sheets_values_dates_repeats_and_merges_are_read() {
    let bytes = sxc(
        b"application/vnd.sun.xml.calc",
        r#"<table:table table:name="Ventas"><table:table-row><table:table-cell><text:p>Producto</text:p></table:table-cell><table:table-cell table:value-type="float" table:value="12.5"><text:p>12,5</text:p></table:table-cell><table:table-cell table:value-type="percentage" table:value="0.15"><text:p>15%</text:p></table:table-cell></table:table-row><table:table-row table:number-rows-repeated="2"><table:table-cell table:number-columns-repeated="2" table:value-type="float" table:value="3"><text:p>3</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell table:value-type="date" table:date-value="2024-03-01"><text:p>01/03/24</text:p></table:table-cell><table:table-cell table:value-type="boolean" table:boolean-value="true"><text:p>TRUE</text:p></table:table-cell><table:table-cell table:formula="=[.B1]*2" table:value-type="float" table:value="25" table:number-columns-spanned="2"><text:p>25</text:p></table:table-cell><table:covered-table-cell/></table:table-row></table:table><table:table table:name="Notas"><table:table-row><table:table-cell><text:p>hola</text:p></table:table-cell></table:table-row></table:table>"#,
    );
    assert_eq!(sniff(&bytes), Format::Ods, "an .sxc package is read by the ODS reader");
    let (wb, report) = read_ods(&bytes).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("SXC (OpenOffice.org 1.x)")), "{:?}", report.warnings);
    assert_eq!(wb.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Ventas", "Notas"]);
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.value(at("A1")), text("Producto"));
    assert_eq!(sh.value(at("B1")), Value::Number(12.5), "values come from table:value, not the displayed text");
    assert_eq!(sh.value(at("C1")), Value::Number(0.15));
    assert_eq!(fmt(&wb, "C1"), "0.00%");
    assert_eq!((sh.value(at("A2")), sh.value(at("B3"))), (Value::Number(3.0), Value::Number(3.0)), "repeats");
    assert_eq!(sh.value(at("A4")), Value::Number(45352.0), "2024-03-01");
    assert_eq!(sh.value(at("B4")), Value::Bool(true));
    assert_eq!(sh.value(at("C4")), Value::Number(25.0), "a formula keeps its saved result");
    assert_eq!(sh.merges, vec![RangeRef::new(at("C4"), at("D4"))]);
    assert_eq!(wb.sheet(1).unwrap().value(at("A1")), text("hola"));
    // The template mimetype is read the same way.
    let template = sxc(
        b"application/vnd.sun.xml.calc.template",
        r#"<table:table table:name="T"><table:table-row><table:table-cell><text:p>x</text:p></table:table-cell></table:table-row></table:table>"#,
    );
    assert_eq!(read_ods(&template).unwrap().0.sheet(0).unwrap().value(at("A1")), text("x"));
}

#[test]
fn sxc_and_ods_namespaces_do_not_mix() {
    // ODF namespaces under an .sxc mimetype (or the reverse) are not read as cells.
    let odf_body = r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:body><office:spreadsheet><table:table table:name="A"/></office:spreadsheet></office:body></office:document-content>"#;
    let bytes = make_zip(&[("mimetype", b"application/vnd.sun.xml.calc"), ("content.xml", odf_body.as_bytes())]);
    assert!(read_ods(&bytes).is_err());
    // An encrypted .sxc (manifest:encryption-data) is refused.
    let encrypted = br#"<manifest:manifest xmlns:manifest="http://openoffice.org/2001/manifest"><manifest:file-entry manifest:full-path="content.xml"><manifest:encryption-data/></manifest:file-entry></manifest:manifest>"#;
    let content = r#"<office:document-content xmlns:office="http://openoffice.org/2000/office" xmlns:table="http://openoffice.org/2000/table"><office:body><table:table table:name="A"/></office:body></office:document-content>"#;
    let bytes = make_zip(&[("mimetype", b"application/vnd.sun.xml.calc"), ("META-INF/manifest.xml", encrypted), ("content.xml", content.as_bytes())]);
    assert!(read_ods(&bytes).unwrap_err().to_string().contains("encrypted"));
}

// ---------------------------------------------------------------- XML Spreadsheet 2003

const XML2003: &str = r##"<?xml version="1.0"?>
<?mso-application progid="Excel.Sheet"?>
<Workbook xmlns="urn:schemas-microsoft-com:office:spreadsheet"
 xmlns:o="urn:schemas-microsoft-com:office:office"
 xmlns:x="urn:schemas-microsoft-com:office:excel"
 xmlns:ss="urn:schemas-microsoft-com:office:spreadsheet"
 xmlns:html="http://www.w3.org/TR/REC-html40">
 <Styles>
  <Style ss:ID="Default" ss:Name="Normal"><Font ss:FontName="Arial" ss:Size="10"/></Style>
  <Style ss:ID="sHead"><Font ss:FontName="Arial" ss:Size="10" ss:Bold="1" ss:Color="#FFFFFF"/><Interior ss:Color="#1F4E78" ss:Pattern="Solid"/><Alignment ss:Horizontal="Center" ss:WrapText="1"/>
   <Borders><Border ss:Position="Bottom" ss:LineStyle="Continuous" ss:Weight="2"/></Borders></Style>
  <Style ss:ID="sDate"><NumberFormat ss:Format="Short Date"/></Style>
  <Style ss:ID="sMoney" ss:Parent="sHead"><NumberFormat ss:Format="#,##0.00"/><Interior/></Style>
 </Styles>
 <Worksheet ss:Name="Movimientos">
  <Table ss:ExpandedColumnCount="4" ss:ExpandedRowCount="5">
   <Column ss:Width="90"/><Column ss:Index="3" ss:Width="60" ss:Span="1"/>
   <Row ss:Height="24">
    <Cell ss:StyleID="sHead"><Data ss:Type="String">Fecha</Data></Cell>
    <Cell ss:StyleID="sHead" ss:MergeAcross="1"><Data ss:Type="String">Detalle</Data></Cell>
    <Cell ss:StyleID="sHead"><Data ss:Type="String">Monto</Data></Cell>
   </Row>
   <Row>
    <Cell ss:StyleID="sDate"><Data ss:Type="DateTime">2024-03-01T00:00:00.000</Data></Cell>
    <Cell><ss:Data ss:Type="String" xmlns="http://www.w3.org/TR/REC-html40"><B>Pago</B> &amp; <I>abono</I></ss:Data><Comment><ss:Data>not the value</ss:Data></Comment></Cell>
    <Cell ss:Index="4" ss:StyleID="sMoney"><Data ss:Type="Number">1500.5</Data></Cell>
   </Row>
   <Row ss:Index="4">
    <Cell><Data ss:Type="Boolean">1</Data></Cell>
    <Cell><Data ss:Type="Error">#DIV/0!</Data></Cell>
    <Cell><Data ss:Type="DateTime">1899-12-31T12:00:00.000</Data></Cell>
    <Cell ss:Formula="=SUM(R[-2]C:R[-1]C)"><Data ss:Type="Number">1500.5</Data></Cell>
   </Row>
  </Table>
  <WorksheetOptions xmlns="urn:schemas-microsoft-com:office:excel">
   <FreezePanes/><FrozenNoSplit/><SplitHorizontal>1</SplitHorizontal><TopRowBottomPane>1</TopRowBottomPane>
   <DoNotDisplayGridlines/>
  </WorksheetOptions>
 </Worksheet>
 <Worksheet ss:Name="Oculta"><Table/><WorksheetOptions xmlns="urn:schemas-microsoft-com:office:excel"><Visible>SheetHidden</Visible></WorksheetOptions></Worksheet>
</Workbook>"##;

/// A stand-in for the engine's R1C1 → A1 conversion, which lives above this crate.
fn convert(f: &str, at: CellRef) -> String {
    format!("{f}@{}", at.a1())
}

#[test]
fn xml_spreadsheet_2003_values_styles_merges_and_options_are_read() {
    assert_eq!(sniff(XML2003.as_bytes()), Format::Xml2003);
    let (wb, report) = read_xml2003(XML2003.as_bytes(), &convert).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("comments")), "{:?}", report.warnings);
    assert_eq!(wb.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Movimientos", "Oculta"]);
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.value(at("A1")), text("Fecha"));
    assert_eq!(sh.value(at("B1")), text("Detalle"));
    assert_eq!(sh.value(at("D1")), text("Monto"), "MergeAcross moves the next cell past the merge");
    assert_eq!(sh.merges, vec![RangeRef::new(at("B1"), at("C1"))]);
    assert_eq!(sh.value(at("A2")), Value::Number(45352.0));
    assert_eq!(fmt(&wb, "A2"), "m/d/yyyy", "Short Date");
    assert_eq!(sh.value(at("B2")), text("Pago & abono"), "rich text keeps its words; the comment's Data is not the value");
    assert_eq!(sh.value(at("D2")), Value::Number(1500.5), "ss:Index skips to column D");
    assert_eq!(fmt(&wb, "D2"), "#,##0.00");
    assert_eq!(sh.value(at("A4")), Value::Bool(true), "ss:Index on the row skips row 3");
    assert_eq!(sh.value(at("B4")), Value::Error(CellError::Div0));
    assert_eq!(sh.value(at("C4")), Value::Number(0.5), "a time alone sits on day zero");
    assert_eq!(sh.value(at("D4")), Value::Number(1500.5));
    let f = sh.cell(at("D4")).and_then(|c| c.formula.as_ref()).map(|f| f.text.clone());
    assert!(f.is_none(), "the stand-in conversion yields text that is not a formula, so only the value is kept");
}

#[test]
fn xml_spreadsheet_2003_styles_sizes_and_sheet_options_are_read() {
    let (wb, _) = read_xml2003(XML2003.as_bytes(), &convert).unwrap();
    let sh = wb.sheet(0).unwrap();
    // Styles: the header style and one that inherits it through ss:Parent.
    let head = wb.styles.get(sh.style_id(at("A1")));
    assert!(head.font.bold);
    assert_eq!(head.font.color, Color::Rgb(0xFFFFFF));
    assert_eq!(head.font.name, "Arial");
    assert_eq!(head.fill.pattern, PatternType::Solid);
    assert_eq!(head.fill.fg, Color::Rgb(0x1F4E78));
    assert_eq!(head.align.h, HAlign::Center);
    assert!(head.align.wrap);
    assert_eq!(head.borders.bottom.style, gridcraft_model::BorderStyle::Medium);
    let money = wb.styles.get(sh.style_id(at("D2")));
    assert!(money.font.bold, "inherited from the parent style");
    assert_eq!(money.fill.pattern, PatternType::None, "an empty Interior clears the parent's fill");
    let plain = wb.styles.get(sh.style_id(at("A4")));
    assert_eq!(plain.font.name, "Arial", "cells without a style take Default");
    // Sizes and options.
    assert!(sh.cols.get(&0).and_then(|i| i.size).is_some_and(|w| w > 100.0), "90 pt");
    assert!(sh.cols.contains_key(&2) && sh.cols.contains_key(&3), "Span repeats the column");
    assert!(!sh.cols.contains_key(&1));
    assert!(sh.rows.get(&0).and_then(|i| i.size).is_some_and(|h| h > 30.0), "24 pt");
    assert_eq!(sh.freeze, Some((1, 0)));
    assert!(!sh.show_gridlines);
    assert_eq!(wb.sheet(1).unwrap().visibility, Visibility::Hidden);
}

#[test]
fn xml_spreadsheet_2003_hostile_input_is_refused_or_bounded() {
    let wrap = |body: &str| {
        format!(
            r#"<?xml version="1.0"?><Workbook xmlns="urn:schemas-microsoft-com:office:spreadsheet" xmlns:ss="urn:schemas-microsoft-com:office:spreadsheet">{body}</Workbook>"#
        )
    };
    // No worksheets, truncated XML, DOCTYPE entities.
    assert!(read_xml2003(wrap("").as_bytes(), &convert).is_err());
    assert!(read_xml2003(&XML2003.as_bytes()[..XML2003.len() / 2], &convert).is_err());
    let doctype = r#"<?xml version="1.0"?><!DOCTYPE x [<!ENTITY a "aaaa">]><Workbook xmlns="urn:schemas-microsoft-com:office:spreadsheet"/>"#;
    assert!(read_xml2003(doctype.as_bytes(), &convert).is_err());
    // Indexes past the grid and huge spans and merges stay inside it.
    let big = wrap(
        r#"<Worksheet ss:Name="S"><Table><Column ss:Span="4294967295" ss:Width="10"/><Row ss:Index="99999999" ss:Span="4294967295" ss:Height="10"><Cell ss:Index="999999"><Data ss:Type="Number">1</Data></Cell></Row><Row ss:Index="1048576"><Cell ss:Index="16384" ss:MergeAcross="4294967295" ss:MergeDown="4294967295"><Data ss:Type="Number">2</Data></Cell><Cell><Data ss:Type="Number">3</Data></Cell></Row></Table></Worksheet>"#,
    );
    let (wb, report) = read_xml2003(big.as_bytes(), &convert).unwrap();
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.value(CellRef::new(1_048_575, 16_383)), Value::Number(2.0));
    assert_eq!(sh.merges, vec![RangeRef::new(CellRef::new(1_048_575, 16_383), CellRef::new(1_048_575, 16_383))]);
    assert!(report.warnings.iter().any(|w| w.contains("outside the worksheet grid")), "{:?}", report.warnings);
    // Plain XML that isn't a spreadsheet, and CSV, are not taken for one.
    assert_ne!(sniff(b"<?xml version=\"1.0\"?><Workbook/>"), Format::Xml2003);
    assert_eq!(sniff(b"a,b\n1,2\n"), Format::Csv);
    // A UTF-16 document is decoded.
    let utf16: Vec<u8> =
        [0xFF, 0xFE].into_iter().chain(XML2003.replace("<?xml version=\"1.0\"?>", "").encode_utf16().flat_map(u16::to_le_bytes)).collect();
    assert_eq!(sniff(&utf16), Format::Xml2003);
    assert_eq!(read_xml2003(&utf16, &convert).unwrap().0.sheet(0).unwrap().value(at("A1")), text("Fecha"));
}
