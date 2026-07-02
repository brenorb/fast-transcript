import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { SpreadsheetFile, Workbook } from "@oai/artifact-tool";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const dataPath = path.join(__dirname, "feature_tracker_data.json");
const outputPath = path.join(__dirname, "feature-tracker.xlsx");
const summaryPreviewPath = path.join(__dirname, "feature-tracker-summary.png");
const featuresPreviewPath = path.join(__dirname, "feature-tracker-features.png");

const statusPalette = {
  headerFill: "#16324F",
  headerFont: "#FFFFFF",
  sectionFill: "#EAF2F8",
  summaryFill: "#F7F9FB",
  neutral: "#6B7280",
};

const readData = async () => {
  const raw = await fs.readFile(dataPath, "utf8");
  return JSON.parse(raw);
};

const applyHeader = (range) => {
  range.format.fill.color = statusPalette.headerFill;
  range.format.font.color = statusPalette.headerFont;
  range.format.font.bold = true;
  range.format.horizontalAlignment = "Center";
  range.format.verticalAlignment = "Center";
  range.format.wrapText = true;
  range.format.rowHeight = 28;
  range.format.borders = { preset: "all", style: "thin", color: "#D6DEE8" };
};

const setColumnWidths = (sheet) => {
  const widths = [
    ["A:A", 14],
    ["B:B", 15],
    ["C:C", 30],
    ["D:D", 38],
    ["E:E", 56],
    ["F:F", 38],
    ["G:G", 12],
    ["H:M", 16],
    ["M:M", 36],
  ];
  for (const [rangeRef, width] of widths) {
    sheet.getRange(rangeRef).format.columnWidth = width;
  }
};

const addSummary = (workbook, totalRows) => {
  const sheet = workbook.worksheets.add("Summary");
  sheet.showGridLines = false;
  sheet.getRange("A1:F1").merge();
  sheet.getRange("A1").values = [["fast-transcript feature audit"]];
  sheet.getRange("A1:F1").format.fill.color = statusPalette.headerFill;
  sheet.getRange("A1:F1").format.font.color = statusPalette.headerFont;
  sheet.getRange("A1:F1").format.font.bold = true;
  sheet.getRange("A1:F1").format.font.size = 16;
  sheet.getRange("A1:F1").format.horizontalAlignment = "Center";
  sheet.getRange("A1:F1").format.rowHeight = 26;

  sheet.getRange("A3:B8").values = [
    ["Goal", "Audit every feature, test every user story, log issues, fix UX/logistical errors, and retest."],
    ["Generated", "2026-07-01"],
    ["Feature rows", totalRows],
    ["Audit status", "In progress"],
    ["Current phase", "Automated verification"],
    ["Canonical artifact", "This workbook"]
  ];
  sheet.getRange("A3:A8").format.font.bold = true;
  sheet.getRange("A3:B8").format.fill.color = statusPalette.summaryFill;
  sheet.getRange("A3:B8").format.wrapText = true;
  sheet.getRange("A3:B8").format.borders = { preset: "all", style: "thin", color: "#D6DEE8" };
  sheet.getRange("A3:B8").format.autofitRows();

  sheet.getRange("D3:E8").values = [
    ["Metric", "Value"],
    ["Audited rows", null],
    ["Tests started", null],
    ["Issues logged", null],
    ["Fixes in progress", null],
    ["Retests complete", null]
  ];
  applyHeader(sheet.getRange("D3:E3"));
  const lastFeatureRow = totalRows + 1;
  sheet.getRange("E4:E8").formulas = [
    [`=COUNTIF(Features!$H$2:$H$${lastFeatureRow},"Audited")`],
    [`=COUNTIF(Features!$I$2:$I$${lastFeatureRow},"<>Not started")`],
    [`=COUNTIF(Features!$J$2:$J$${lastFeatureRow},"<>None logged")`],
    [`=COUNTIF(Features!$K$2:$K$${lastFeatureRow},"In progress")`],
    [`=COUNTIF(Features!$L$2:$L$${lastFeatureRow},"Complete")`]
  ];
  sheet.getRange("D4:E8").format.fill.color = "#FFFFFF";
  sheet.getRange("D4:E8").format.borders = { preset: "all", style: "thin", color: "#D6DEE8" };
  sheet.getRange("D4:D8").format.font.bold = true;
  sheet.getRange("A:A").format.columnWidth = 18;
  sheet.getRange("B:B").format.columnWidth = 52;
  sheet.getRange("D:E").format.columnWidth = 18;
  sheet.freezePanes.freezeRows(3);
  return sheet;
};

const addFeatures = (workbook, rows) => {
  const sheet = workbook.worksheets.add("Features");
  sheet.showGridLines = false;

  const headers = [[
    "Feature ID",
    "Area",
    "Title",
    "User Story",
    "Expected Behavior",
    "Evidence",
    "Coverage",
    "Audit Status",
    "Test Status",
    "Issue Status",
    "Fix Status",
    "Retest Status",
    "Notes"
  ]];
  sheet.getRange("A1:M1").values = headers;
  applyHeader(sheet.getRange("A1:M1"));

  const matrix = rows.map((row) => [
    row.feature_id,
    row.area,
    row.title,
    row.user_story,
    row.expected_behavior,
    row.evidence,
    row.coverage,
    row.audit_status,
    row.test_status,
    row.issue_status,
    row.fix_status,
    row.retest_status,
    row.notes
  ]);
  if (matrix.length > 0) {
    sheet.getRange(`A2:M${rows.length + 1}`).values = matrix;
  }

  sheet.getRange(`A2:M${rows.length + 1}`).format.wrapText = true;
  sheet.getRange(`A2:M${rows.length + 1}`).format.verticalAlignment = "Top";
  sheet.getRange(`A2:M${rows.length + 1}`).format.borders = {
    preset: "all",
    style: "thin",
    color: "#E5E7EB"
  };
  sheet.getRange(`A2:B${rows.length + 1}`).format.fill.color = "#F9FBFC";
  sheet.getRange(`G2:L${rows.length + 1}`).format.horizontalAlignment = "Center";
  sheet.getRange(`G2:L${rows.length + 1}`).dataValidation = {
    rule: {
      type: "list",
      values: [
        "Yes",
        "Partial",
        "No",
        "Audited",
        "Not started",
        "In progress",
        "Blocked",
        "Passed",
        "Failed",
        "Resolved",
        "None logged",
        "Not needed",
        "Complete"
      ]
    }
  };

  setColumnWidths(sheet);
  sheet.getRange(`A1:M${rows.length + 1}`).format.autofitRows();
  sheet.freezePanes.freezeRows(1);
  sheet.freezePanes.freezeColumns(2);
  return sheet;
};

const main = async () => {
  const data = await readData();
  const workbook = Workbook.create();
  addFeatures(workbook, data.rows);
  addSummary(workbook, data.rows.length);
  await fs.mkdir(__dirname, { recursive: true });
  const summaryPreview = await workbook.render({
    sheetName: "Summary",
    range: "A1:F8",
    scale: 2,
    format: "png"
  });
  await fs.writeFile(summaryPreviewPath, new Uint8Array(await summaryPreview.arrayBuffer()));
  const featuresPreview = await workbook.render({
    sheetName: "Features",
    range: "A1:M8",
    scale: 1.5,
    format: "png"
  });
  await fs.writeFile(featuresPreviewPath, new Uint8Array(await featuresPreview.arrayBuffer()));
  const output = await SpreadsheetFile.exportXlsx(workbook);
  await output.save(outputPath);
  console.log(outputPath);
};

await main();
