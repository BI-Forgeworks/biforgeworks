# Power BI Project fixtures

Minimal, synthetic Power BI Projects used by `crates/biforgeworks-powerbi`
tests. They contain only the outer metadata and structural markers that
discovery inspects; report and model files are placeholders, not real
content. No customer data, credentials, or connection strings.

| Fixture           | Entry file                | Expected report | Expected model | Notes                                              |
| ----------------- | ------------------------- | --------------- | -------------- | -------------------------------------------------- |
| `valid-pbir-tmdl` | `Sales.pbip`              | `PBIR`          | `TMDL`         | `definition.pbir` 4.0, `definition.pbism` 4.2      |
| `valid-legacy`    | `Legacy.pbip`             | `PBIR_LEGACY`   | `TMSL`         | `definition.pbir` 1.0, `definition.pbism` 1.0      |
| `missing-report`  | `MissingReport.pbip`      | `MISSING`       | `MISSING`      | report folder absent (`REPORT_FOLDER_NOT_FOUND`)   |
| `missing-model`   | `MissingModel.pbip`       | `PBIR`          | `MISSING`      | model folder absent (`SEMANTIC_MODEL_FOLDER_NOT_FOUND`) |
| `malformed-pbip`  | `Malformed.pbip`          | `MISSING`       | `MISSING`      | truncated JSON (`PBIP_INVALID_JSON`)               |
| `unknown-formats` | `Unknown.pbip`            | `UNKNOWN`       | `UNKNOWN`      | valid metadata, `definition/` folders lack markers |

Tests never modify these files; projects that need mutation (symlinks,
FIFOs, stale timestamps, oversized files) are built in temporary
directories.
