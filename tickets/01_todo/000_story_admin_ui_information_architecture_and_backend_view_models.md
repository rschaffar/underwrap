# Story: Define the new admin UI information architecture and backend view-model APIs

Epic: `tickets/01_todo/epic_nextjs_admin_ui_replatform.md`

## Background

The current UI composes pages by fetching broad backend primitives
(policy document, catalog, secret id lists) and then building the page
shape in the UI layer. That is workable for the current form-based Rust
UI, but it is the wrong foundation for a richer Next.js operator
console.

Because this project is greenfield, we should reshape the backend admin
API where that gives us cleaner page models and less accidental frontend
complexity.

## Scope

- Define the page map and primary operator workflows, including first-class
  multi-admin target switching and a read-only audit/history page.
- Define the frontend-facing backend API/read models needed for:
  - dashboard / overview
  - resource search / listing
  - resource detail
  - wrapper list / detail
  - access matrix view
  - audit history view
  - secret reveal
  - mutation state visibility
- Decide whether these endpoints live under a dedicated namespace such
  as `/v1/admin/ui/*` or another equivalent shape.
- Define filtering, sorting, pagination, and response payload rules.
- Define secret reveal and file transfer semantics for:
  - UTF-8 text
  - binary / non-UTF-8 material
  - real file upload/download
  - a 1 MiB per-file limit in v1
- Document the contract in markdown before implementation starts.

## Acceptance criteria

- [ ] A written API/UX design document exists in `docs/`.
- [ ] Primary pages and workflows are named and scoped.
- [ ] Required backend view-model and audit endpoints are listed with
      request and response shapes.
- [ ] Reveal/upload/download semantics are explicitly defined for text
      vs. binary material and the 1 MiB file cap.
- [ ] The design avoids direct browser calls to `backend-backendd`.
- [ ] The design is good enough to let implementation tickets proceed
      without page-model ambiguity.

## Child tasks

- `000_task_admin_ui_rfc_page_map_and_endpoint_contract.md`
- `060_task_admin_ui_backend_overview_and_resource_index_endpoints.md`
- `070_task_admin_ui_backend_resource_detail_reveal_and_binary_download_endpoints.md`
- `080_task_admin_ui_backend_wrapper_matrix_and_mutation_view_endpoints.md`
