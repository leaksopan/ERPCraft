# Inventory and fixed assets

These follow Business Central's W1 behaviour (BCApps `Inventory` and `FixedAssets`).

## Locations

- `Location` (table 14).
- **Location Code** is a field on customers, vendors, sales and purchase headers and lines, and item ledger entries.
- A document takes its customer's or vendor's location. Lines take the header's. Changing the header's location updates lines that have not been shipped yet.
- **Inventory Posting Setup** is per location and inventory posting group. Without a row for the location, the blank-location row is used.
- **Item Availability** shows stock per location.
- **Inventory Setup › Prevent Negative Inventory** refuses any shipment, negative adjustment or transfer that would take more than a location holds. The message is BC's: *"You have insufficient quantity of Item … on inventory"*.

## Item journals

| Page | Template | Type | Number series |
| --- | --- | --- | --- |
| Item Journals (40) | `ITEM` | Item | IJ00001… |
| Item Reclass. Journals (393) | `RECLASS` | Transfer | IR00001… |
| Phys. Inventory Journals (392) | `PHYS INV` | Phys. Inventory | IP00001… |

What validating a line does:
- **Item No.:** fills the description, unit, Unit Amount (the unit cost) and posting groups. Only inventory items are allowed.
- **Quantity:** Amount = Quantity × Unit Amount.
- **Qty. (Phys. Inventory):** sets the entry type and quantity from the difference to Qty. (Calculated).

What `item_journal.post` (F9) writes per entry type:

| Entry type | Item ledger | G/L |
| --- | --- | --- |
| Positive Adjmt. | + quantity at the location, cost = amount. The item's average cost is updated. | Dr Inventory, Cr Inventory Adjmt. Account (General Posting Setup) |
| Negative Adjmt. | − quantity | Dr Inventory Adjmt. Account, Cr Inventory |
| Transfer | − at Location Code, + at New Location Code | Only when the two locations use different inventory accounts |

Purchase and Sale entries go through purchase and sales documents, as in BC.

**Stock count (opname):**
1. **Calculate Inventory** (`item_journal.calculate_inventory`) makes one line per item and location holding stock, or per inventory item for one location.
2. Enter the counted quantity in Qty. (Phys. Inventory).
3. Post. Only differences are posted, and every line of the batch is cleared.

The **Inventory Valuation** report gives quantity and value per item, next to the inventory G/L balance. In the demo they match.

## Fixed assets

Tables:
- Fixed Asset (5600)
- FA Depreciation Book (5612)
- FA Ledger Entry (5601)
- FA Posting Group (5606)
- Depreciation Book (5611)
- FA Class, FA Subclass and FA Location (5607–5609)
- FA Setup (5603)

**The card.** A new fixed asset gets its number from FA Setup (FA000010, FA000020, …). It also gets a depreciation book row for the default book (`COMPANY`). The FA Subclass fills the FA Class and the FA Posting Group. On the book, No. of Depreciation Years sets the ending date (start + years − 1 day).

**Acquisition.** Each way of getting an asset is a choice of balancing account, as metta's asset receipt has *pembelian*, *hibah* and *saldo awal*:

| How the asset came | Posting |
| --- | --- |
| Purchased (invoice) | Purchase invoice line of type Fixed Asset: Dr Acquisition Cost Account plus VAT, Cr Payables |
| Purchased (paid directly) | FA G/L Journal, FA Posting Type Acquisition Cost, balanced against the bank |
| Grant (hibah) | FA G/L Journal, balanced against a grant account (demo: 20400 Deferred Grant Income) |
| Opening balance (saldo awal) | FA G/L Journal, balanced against equity (demo: 30200 Owner's Capital) |

The first acquisition marks the asset Acquired, and fills the book's Acquisition Date and, if empty, its Depreciation Starting Date.

**Depreciation.** `fa.calculate_depreciation {date, post, fa_no}` works like Calculate Depreciation on the Fixed Assets page:
- The method is straight-line on BC's 360-day year (`DeprDays`, both dates inclusive, the 31st counts as the 30th). Every month counts as 30 days, February included.
- The amount for the period is acquisition cost × days ÷ (years × 360). The last period takes what is left, and the book value never goes below zero.
- Lines go into the Fixed Asset G/L Journal (template `ASSETS`), balanced against the posting group's Depreciation Expense Acc. With `post: Yes` they are posted at once.
- On posting: Dr Depreciation Expense, Cr Accumulated Depreciation, plus an FA ledger entry of type Depreciation with its number of depreciation days.

**Disposal.** An FA G/L Journal line with FA Posting Type Disposal, amount = −proceeds, balanced against the bank:
- **G/L:** removes the acquisition cost and accumulated depreciation, and posts the difference between proceeds and book value to Gains or Losses Acc. on Disposal.
- **FA ledger:** Proceeds on Disposal, Gain/Loss and Book Value on Disposal entries. The book value becomes 0 and the book gets a Disposal Date.
- **Afterwards:** nothing more can be posted to the asset.

The **Fixed Asset Book Value** report shows acquisition cost, depreciation and book value per asset. Its total equals Fixed Assets (10500) + Accumulated Depreciation (10510) in the G/L.

## Demo data

- **Locations:** MAIN (Gudang Pusat Jakarta) and BANDUNG. Ten tires were moved to Bandung with the reclassification journal.
- **Opening balance:** Rp 1.5 billion of owner's capital into the operating bank account.
- **Fixed assets:** four, each depreciated up to the end of last month:

  | No. | Asset | How it was acquired | Amount |
  | --- | --- | --- | --- |
  | FA000010 | Tow truck | Bank | Rp 385 million |
  | FA000020 | Car lift | Grant | Rp 95 million |
  | FA000030 | Diagnostic scanner | Opening balance | Rp 28 million |
  | FA000040 | Air compressor | Purchase invoice | Rp 18.5 million |

## Not yet

- Declining-balance and manual depreciation.
- Write-down, appreciation and maintenance postings.
- Insurance.
- More than one depreciation book per asset in the UI.
- Reversing FA transactions (post a correcting line instead).
- Bins and warehouse documents.
- Transfer orders.
- Item tracking (serial and lot numbers).
