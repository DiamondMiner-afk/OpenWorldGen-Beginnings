# Source inventory

## Active synthetic inputs

| Source | Role | Licence/confidentiality | Status |
| --- | --- | --- | --- |
| `data/synthetic-balanced-v0.1/data-bundle.json` | Synthetic generator metadata and units | internal-research / public-synthetic | contract-valid, development |
| `model-packs/small-open-economy-v0.1/model-pack.json` | Behavioral, market, accounting, and schedule definition | proprietary / internal | contract-valid, development |
| Preregistered scenario | Shock and policy branches | proprietary / internal | executable |

## India pilot candidates (gate-locked)

| Institution/source | Intended target | Pre-ingestion requirement |
| --- | --- | --- |
| MoSPI National Accounts and 2022-23/2023-24 Supply and Use Tables | production, expenditure, input-output structure | classification and base-year bridge |
| HCES 2023-24 | consumption baskets and distribution | disclosure, weight, and unit audit |
| PLFS unit-level data | employment, wages, transitions, skills | access/licence review and survey-design handling |
| ASI and ASUSE | firm/establishment size and sector distributions | establishment-to-firm bridge |
| RBI DBIE | prices, policy rates, credit, fiscal, external, confidence, expectations | vintage/retrieval logging and series dictionary |

## US historical pilot: first archived source

The Philadelphia Fed Real-Time Data Set workbooks for real output (ROUTPUT),
unemployment (RUC), and CPI were downloaded on 2026-10-17. They contain the
full archived quarterly-vintage matrix. The project extracts the 2006Q4
pre-period origin and all 2007Q1–2023Q4 quarterly vintages into separate,
hash-checked point-in-time CSVs. See
`research/us-economy-2007-2023/notes/REAL_DATA_AND_LOCAL_LLM.md` for exact
files, original download links, units, timing, quality checks, and limitations.
These macro series do not calibrate the agent economy by themselves.

The archived March and June 2007 and March and June 2023 Federal Reserve Z.1
release ZIPs and PDFs are hash-checked. Two household/nonprofit deposit series
can be read from both PRN and CSV formats and aligned by series ID, label,
unit, and observation quarter. Same-quarter values differ between surviving
release-labelled files; the 2007 files' post-release modification dates mean
these are **not** authenticated release-day actor inputs. Sector-footnote and
stock-counterparty questions remain open. This is a **candidate measurement
bridge**, not a calibrated financial opening state. See
`research/us-economy-2007-2023/notes/FED_Z1_HOUSEHOLD_DEPOSITS_BRIDGE.md`
and `research/us-economy-2007-2023/notes/FED_Z1_ADJACENT_HOUSEHOLD_DEPOSITS_PROBE.md`.

The earlier [December 7, 2006 Z.1 release](https://www.federalreserve.gov/releases/z1/20061207/data.htm)
was also archived as the relevant pre-January-2007 *release-level* candidate.
Its Q3 2006 household/nonprofit checkable-deposits-and-currency and
time-and-savings-deposit levels were parsed from the PRN ZIP and rounded
against the printed PDF. The same Q3 codes differ substantially in the
surviving March 2007 file. Both current December-labelled files have March
2007 HTTP modification dates, so neither exact file is certified as January
2007 actor information. See
`research/us-economy-2007-2023/notes/FED_Z1_DECEMBER_2006_PREORIGIN_PROBE.md`.

BEA's [December 2006 annual-industry account](https://apps.bea.gov/scb/pdf/2006/12december/1206_indyaccts.pdf)
states that its December 11 update incorporated 2005 annual I-O accounts;
the 2006 annual I-O update was still scheduled for December 2007 in a later
[BEA release](https://www.bea.gov/news/2007/benchmark-input-output-accounts-us-economy-2002).
A 2005 make/use workbook was recovered from a June 5, 2007 capture of the BEA
URL and archived with the same SHA-256 as the currently BEA-hosted file. BEA
release-level availability by January 2007 is supported, but this workbook's
**exact bytes are not certified as available by that cutoff**; the earliest
independent byte capture found is June 2007. Direct parsing has verified all
8,840 numeric Make/Use cells and 693 displayed-detail accounting comparisons;
cross-table published output totals match, while the largest detail residual
is $0.7 million. A four-sector classification/aggregation candidate exists,
but exact January-2007 byte certification remains open. A reviewable four-sector
**candidate** now accounts for all 65 ordinary codes and keeps signed imports,
scrap, and inventory-valuation adjustments outside ordinary Use and Make
matrices. Low-confidence assignments—especially real estate, finance,
construction, and government—preclude treating it as a calibrated supplier
network. See
`research/us-economy-2007-2023/notes/BEA_IO_2005_SOURCE_PROBE.md` and
`research/us-economy-2007-2023/notes/BEA_IO_2005_MATRIX_QC.md` and
`research/us-economy-2007-2023/notes/BEA_IO_2005_FOUR_SECTOR_CANDIDATE_V1.md`.
The ordinary Use table includes imported content, so a matched, dated
before-redefinitions import matrix is required to infer domestic use; `F050`
alone cannot allocate imports to industry buyers. None has yet been verified
for the January 2007 actor cutoff. See
`research/us-economy-2007-2023/notes/BEA_DOMESTIC_IMPORT_BRIDGE_GATE.md`.
Current revised BEA I-O series are not interchangeable with a 2006
publication vintage.

A bounded archived BEA page explicitly labels a tempting 2002–2005 import
workbook **after redefinitions**, so it is incompatible with the audited
**before-redefinitions** Make/Use table. The project separately archived an
[official 1997–2019 before-redefinitions import workbook](https://apps.bea.gov/industry/xls/io-annual/ImportMatrices_Before_Redefinitions_SUM_1997-2019.xlsx)
as **retrospective evaluation-only** evidence. Its 2020 Annual Update uses
2012 NAICS and a 71-industry 2005 sheet, not the recovered older 65-code
1997-NAICS source. It cannot be used as January 2007 actor knowledge or
subtracted cellwise from that workbook. See
`research/us-economy-2007-2023/notes/BEA_IMPORT_1997_2019_RETROSPECTIVE_PROBE.md`.
Saved, bounded [Internet Archive CDX](https://github.com/internetarchive/wayback/blob/master/wayback-cdx-server/README.md)
responses place the first successful indexed capture of the recovered BEA
Make/Use URL in June 2007. The separate bounded queries returned no
status-200 record for the exact SCF article URL in 2005–2007 or the exact
December 2006 Z.1 ZIP URL in 2006–2007. These URL-specific results are not exhaustive
proof of historical absence; see
`research/us-economy-2007-2023/notes/ARCHIVE_ORIGIN_CDX_PROBE_2026-10-20.md`.

A later alias search recovered [December 16, 2006 Z.1 PDF replay bytes](https://web.archive.org/web/20061216235414id_/http://www.federalreserve.gov/releases/z1/Current/z1.pdf)
matching the saved December-labelled PDF, plus December 2006 archived HTML
releases for BLS CPI, employment, and PPI. Local SHA-1 digests match selected
CDX rows; the Z.1 replay names a December 2006 crawl container. These are
candidate pre-origin bytes, not an independently admitted or complete January
2007 source set. The mutable `Current`/`nr0.htm` URLs, linked tables, release
definitions, and archive-origin records require further checks. See
`research/us-economy-2007-2023/notes/PREORIGIN_BLS_Z1_ARCHIVE_LEADS_2026-10-21.md`.

The full study now has **70 separate Z.1 release packages** from the
[official release calendar](https://www.federalreserve.gov/releases/z1/release-dates.htm),
covering reported 2006 Q3–2023 Q4. The local audit checks 296 files
(543,878,815 bytes), ZIP CRCs, and 70 PDF cover dates/quarters. These remain
current historical downloads with possible later corrections. Directory
`20170305` actually contains a PDF published March 9, 2017; publication dates
are recorded separately from URL path IDs. See
`research/us-economy-2007-2023/notes/Z1_RELEASE_COLLECTION_2007_2023.md`.

A December 2006 extraction now covers 64 summary/sector/instrument tables,
318,207 historical cells, 15,983 archived-PDF value checks, and seven
issuer/holder margins. It preserves the published asset/liability bridge,
unallocated-claim discrepancies, negative net positions, and missing values.
Those margins do not identify individual counterparties or initialize an
empirical engine. See
`research/us-economy-2007-2023/notes/Z1_FINANCIAL_ACCOUNTS_2006_EXTRACTION.md`.

The Federal Reserve's [2004 SCF Bulletin article](https://www.federalreserve.gov/econres/files/2004_bull0206.pdf)
was published in February 2006 and supplies bounded pre-2007 *published*
household-wealth anchors: actual 2004 net-worth percentile cutoffs in its
footnote 9 and conditional median/mean wealth within groups in Table 3. A
hash-checked extraction reproduces the published all-family mean from the
wealth-group means. The present PDF has a 2017 HTTP modification date; its
exact bytes are not certified as January 2007 actor information. This is a
weighted, multiply imputed 2004 survey, not a measured 2007 joint household
distribution or a direct Z.1 sector counterpart. The [current 2004 SCF page](https://www.federalreserve.gov/econres/scf_2004.htm)
links later-updated microdata and 2026 summary extracts, which must not be
silently backdated. See
`research/us-economy-2007-2023/notes/SCF_2004_BULLETIN_WEALTH_PROBE.md`.

Official BLS archived releases identify January-2007 *publication-level*
labor candidates: [November 2006 employment](https://www.bls.gov/news.release/archives/empsit_12082006.pdf),
[2006 Q3 usual weekly earnings](https://www.bls.gov/news.release/archives/wkyeng_10192006.pdf),
and [2005 union membership/representation](https://www.bls.gov/news.release/archives/union2_01202006.pdf).
The 2006 union results were published only on January 25, 2007 and cannot
enter a January 1 actor state. BLS blocked direct PDF retrieval from this
host, so no local source bytes or SHA-256 certification exist yet. The exact
coverage, units, and retrieval gate are in
`research/us-economy-2007-2023/notes/BLS_LABOR_JAN2007_SOURCE_SCOUT.md`.

## US historical pilot candidates (priority after synthetic gate)

| Institution/source | Intended target | Pre-ingestion requirement |
| --- | --- | --- |
| BEA supply/use and national accounts | supplier links, sector output, demand, income | historical release archive, industry bridge, units and base-year check |
| Federal Reserve Z.1 and SCF | sector balance sheets, household wealth and debt | release vintages, survey weights and imputations, stock-flow bridge |
| Census Business Dynamics Statistics | firm entry, exit, size and age | first released in 2008, so evaluation-only for a January 2007 origin; later origins need dated editions, coverage and disclosure flags |
| BLS CPS and union affiliation | employment, earnings, union membership | survey definitions and vintage review |
| SEC EDGAR | public-company disclosures and ownership evidence | filing-date cutoff, survivorship and private-firm coverage |
| Philadelphia Fed Real-Time Data Set | historical macro series available to past decision makers | vintage-column date and original-release documentation |

The pilot's source-selection and falsification workflow is in
`research/us-economy-2007-2023/notes/MECHANISM_REGISTER.md`. The `data-vintage`
v1 contract verifies the extracted release files and rejects post-origin
publications. The opt-in v2 exact-byte gate separately rejects publication-
only and post-cutoff capture claims; its CLI checks self-declared provenance
structure and hashes, **not** independent archive origin. See
`docs/HISTORICAL_INPUT_GATE.md`. No US structural source has been calibrated.

No real-data ingestion is authorized as “calibration” until the synthetic gate
closes. Public availability does not remove licence, revision-vintage, or
confidentiality obligations.
