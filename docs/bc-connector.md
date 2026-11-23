# Business Central API connector

ERPCraft can read and write master data of a Microsoft Dynamics 365 Business Central (SaaS)
company through the standard **API v2.0**
(<https://learn.microsoft.com/dynamics365/business-central/dev-itpro/api-reference/v2.0/>),
signing in with **OAuth2 client credentials** (service-to-service, no user sign-in).

Code: `crates/engine/src/bc/` (`mod.rs` commands and metadata, `map.rs` field mapping,
`client.rs` HTTP). Tests: `crates/engine/tests/bc.rs` (an in-test HTTP server plays Entra ID and
the BC API). The HTTP client sits behind the engine feature `bc` (on by default); a build without
it (`--no-default-features`, the wasm build) has the same commands, which answer
"This build of ERPCraft has no Business Central connector".

## 1. Register an app in Microsoft Entra ID

1. Azure portal → **Microsoft Entra ID** → **App registrations** → **New registration**.
   Name it (e.g. `ERPCraft connector`), single tenant, no redirect URI. Note the
   **Application (client) ID** and the **Directory (tenant) ID**.
2. **Certificates & secrets** → **New client secret**. Copy the secret *value* (shown once).
3. **API permissions** → **Add a permission** → **Dynamics 365 Business Central** →
   **Application permissions** → tick **API.ReadWrite.All** and **Automation.ReadWrite.All** →
   **Add permissions**, then **Grant admin consent for &lt;tenant&gt;**.
4. In Business Central, open the page **Microsoft Entra Applications** → **New**: paste the
   Client ID, give it a description, set **State = Enabled**, and add permission sets under
   **User Permission Sets** (e.g. `D365 BUS FULL ACCESS`, or narrower sets that cover customers,
   vendors, items and G/L accounts) for the company/companies the connector may use. Choose
   **Grant Consent** if BC asks for it.

## 2. Configure ERPCraft

The secret is **never stored** in the company (not in the database, the data file or the log).
Set it in the environment of the process that runs the command (desktop app, `erpcraft-cli` or the
MCP server):

```sh
# PowerShell
$env:ERPCRAFT_BC_CLIENT_SECRET = '<secret value>'
# sh
export ERPCRAFT_BC_CLIENT_SECRET='<secret value>'
```

Everything else lives in the setup table **BC Connection Setup** (object 50100; page
*Setup → BC Connection Setup* in the Business Manager role center), or the `bc.setup` command:

| Field | Meaning |
|---|---|
| Tenant ID | Entra tenant id (GUID) or domain (`contoso.onmicrosoft.com`). Required. |
| Environment | BC environment name. Blank = `Production`. |
| Company ID | The BC company's `id` (GUID). Run `bc.test_connection` to list them. Required for import/export. |
| Company Name | Free text for your reference. |
| Client ID | Application (client) id of the Entra app. Required. |
| API Base URL | Blank = `https://api.businesscentral.dynamics.com/v2.0`. Override for tests / proxies. |
| Token URL | Blank = `https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token` (`{tenant}` is replaced). |
| Last Import At / Last Export At | Set by the commands (UTC). |

Both URLs must be `https://`, except a server on this machine (`http://127.0.0.1`, `localhost`,
`[::1]`), which the tests use.

```sh
erpcraft-cli run bc.setup '{"tenant_id":"contoso.onmicrosoft.com","client_id":"<app id>","environment":"Production"}'
erpcraft-cli run bc.test_connection
erpcraft-cli run bc.setup '{"company_id":"<company id from the list>","company_name":"Acme Trading Ltd."}'
```

## 3. Commands

| Command | Mutates | What it does |
|---|---|---|
| `bc.setup` | yes | Sets setup fields: `tenant_id`, `environment`, `company_id`, `company_name`, `client_id`, `api_base_url`, `token_url`. Refuses `client_secret`. |
| `bc.test_connection` | no | Gets a token, `GET {base}/{tenant}/{environment}/api/v2.0/companies`; returns `companies` (`id`, `name`, `displayName`) and whether the configured Company ID is one of them. |
| `bc.import` | yes | `{"entity": "customers"\|"vendors"\|"items"\|"accounts", "limit"?: n}`. Reads `…/companies({companyId})/{entity}` following `@odata.nextLink` and creates or updates ERPCraft records by No. |
| `bc.export` | yes | `{"entity": "customers"\|"vendors"\|"items", "no"?: "10000", "limit"?: n}`. Reads the BC entity set once, then POSTs records BC doesn't have and PATCHes (with `If-Match: <@odata.etag>`) only the changed properties of records it has. |

```sh
erpcraft-cli run bc.import '{"entity":"customers"}'
erpcraft-cli run bc.import '{"entity":"items","limit":500}'
erpcraft-cli run bc.import '{"entity":"accounts"}'
erpcraft-cli run bc.export '{"entity":"customers"}'
erpcraft-cli run bc.export '{"entity":"items","no":"1000"}'
```

Import and export return

```json
{"entity": "customers", "read": 120, "created": 3, "updated": 5, "unchanged": 111,
 "skipped": 1, "errors": ["Customer C-9: …"], "truncated": false}
```

`errors` holds the first 20 messages; `skipped` counts every record that was not written.
Each run adds an entry to **BC Sync Log** (object 50101). The setup card has actions for all of
the above.

### Transactions and failures

- A command runs as one ERPCraft transaction. A request that fails as a whole (sign-in, network,
  HTTP 401/403, a broken page) makes the command fail and changes nothing locally.
- Within an import, one bad record (say a value that fails validation) is skipped and reported; a
  new record is fully validated before it is inserted, so a failed one leaves nothing behind.
- Export writes to BC as it goes: records exported before a fatal error (401/403, network, 5xx)
  stay exported in BC, while the local Last Export At / log entry are rolled back. Rerunning is
  safe: unchanged records are skipped.

## 4. Field mapping

`number` ↔ `No.` for every entity. Text longer than the ERPCraft field is cut to the field length
(by characters, never inside a UTF-8 sequence). A `number` longer than 20 characters is skipped
rather than cut (cutting a key could merge two records). Values of the wrong JSON type and option
values ERPCraft doesn't know are ignored. Code fields are upper-cased.

### customers → Customer / vendors → Vendor

| API property | ERPCraft field | Notes |
|---|---|---|
| number | No. | key |
| displayName | Name | 100 |
| addressLine1 | Address | 100 |
| addressLine2 | Address 2 | 50 |
| city | City | 30 |
| postalCode | Post Code | 20 |
| country | Country/Region Code | 10 |
| phoneNumber | Phone No. | 30 |
| email | E-Mail | 80 |
| website | Home Page | 80 |
| taxRegistrationNumber | VAT Registration No. | 20 |
| blocked | Blocked | customers `" "`/`Ship`/`Invoice`/`All`; vendors `" "`/`Payment`/`All` (`" "` ↔ blank) |
| creditLimit | Credit Limit (LCY) | customers only |

Not mapped yet: `paymentTermsId`, `paymentMethodId`, `shipmentMethodId`, `currencyCode`,
`salespersonCode`, `taxAreaId`, `type` (Person/Company).

### items → Item

| API property | ERPCraft field | Notes |
|---|---|---|
| number | No. | key |
| displayName | Description | 100 |
| type | Type | `Inventory`, `Service`, `Non_Inventory` ↔ `Non-Inventory` |
| baseUnitOfMeasureCode | Base Unit of Measure | a missing Unit of Measure is created on import |
| unitPrice | Unit Price | |
| unitCost | Unit Cost | |
| blocked | Blocked | |

Not mapped: `itemCategoryCode`, `gtin`, `inventory` (stock comes from item ledger entries),
`taxGroupCode`, `priceIncludesTax`, `generalProductPostingGroupCode`, `inventoryPostingGroupCode`.

### accounts → G/L Account (import only)

| API property | ERPCraft field | Notes |
|---|---|---|
| number | No. | key |
| displayName | Name | 100 |
| category | Account Category | `Assets`, `Liabilities`, `Equity`, `Income`, `CostofGoodsSold` → `Cost of Goods Sold`, `Expense`; also sets Income/Balance |
| subCategory | — | ignored |
| accountType | Account Type | `Posting`, `Heading`, `Total`, `Begin_Total`, `End_Total` |
| directPosting | Direct Posting | |
| blocked | Blocked | |

### Posting groups of new records

The API v2.0 entities don't carry posting groups for customers and vendors. A record created by an
import copies these fields from the **first existing record** of its table (in key order), when
there is one; otherwise they stay blank and must be filled in before the record is used in a
posting:

- Customer: Gen. Bus. Posting Group, VAT Bus. Posting Group, Customer Posting Group
- Vendor: Gen. Bus. Posting Group, VAT Bus. Posting Group, Vendor Posting Group
- Item: Gen. Prod. Posting Group, VAT Prod. Posting Group, Inventory Posting Group

Existing records keep their posting groups on update.

## 5. Limits and security

- At most 50,000 records per import/export (`limit` lowers it) and 2,000 pages; the result says
  `"truncated": true` when a cap cut the list short. Each reply may be up to 64 MB; each request
  times out after 120 s. No automatic retry on HTTP 429 / 5xx: run the command again.
- One token per command (tokens last about an hour; a command never runs that long in practice).
- `@odata.nextLink` is followed only when it points under the configured API root, so the bearer
  token never goes to another host. Redirects are not followed.
- The Company ID and record ids returned by BC must be GUIDs before they are put into a URL; tenant
  and environment are percent-encoded.
- Error messages show the HTTP status and BC's error code/message (cut to 400 characters) and are
  scrubbed of the secret and the token. Nothing is logged except the sync summary.
- Export compares values with what BC holds and PATCHes only the differences, so BC fields
  ERPCraft doesn't map are left alone. The ETag guards against overwriting a record changed in BC
  since it was read; a 412 is reported for that record.
- Sync is by `number` only: a record renamed (renumbered) on one side becomes a new record on the
  other. Deletions are not synchronised.
