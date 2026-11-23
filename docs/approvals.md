# Approvals

ERPCraft provides document approval workflows with an approver chain and configurable approval limits.

## Setup

- **Workflows** holds the approval workflow templates for each sales and purchase document type (for example sales order and purchase invoice approval workflows). A new company starts with them disabled; enabling one puts that document type under approval.
- **User Setup** (page 119) gives each user:
  - an Approver ID;
  - sales and purchase approval limits, or an Unlimited flag;
  - a Substitute;
  - an Approval Administrator flag.
  Users must also exist in Users with permissions.

## Flow

1. **Send Approval Request** on the document creates Approval Entries for the approver chain:
   - the sender's approver first;
   - then each approver's own approver, while the document amount is over the current approver's limit.

   The first entry is Open and the rest are Created. The document's status becomes Pending Approval.
2. **Requests to Approve** (page 654) lists the entries.
   - **Approve** opens the next entry, or releases the document when the last one is approved.
   - **Reject** reopens the document.
   - **Delegate** passes the entry to the approver's substitute.

   Only the entry's approver, or an approval administrator, can act on it.
3. **Cancel Approval Request** withdraws the open entries and reopens the document.
4. **Approvals** on the document lists its approval entries.

While a workflow is enabled, a manual Release refuses with "This document can only be released when the approval process is complete.". Posting refuses until the document is released by approval, with "Sales Order … must be approved and released before you can perform this action.".

## Not covered yet

- Workflow steps other than approval: notifications, response options and approval by user groups.
- Approval comments.
- Approvals for journals and master data.
