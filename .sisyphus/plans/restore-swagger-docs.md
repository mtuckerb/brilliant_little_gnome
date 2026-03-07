# Restore Swagger Documentation

## TL;DR

> **Quick Summary**: Recover deleted Swagger documentation from git history, or failing that, rebuild it using a static OpenAPI spec and Swagger UI.
> 
> **Deliverables**:
> - Restored or new `public/openapi.yaml` (or `.json`)
> - Restored or new `views/docs.erb` (Swagger UI wrapper)
> - Functional `/docs` route
> 
> **Estimated Effort**: Medium (Search is quick; Rebuild is involved)
> **Parallel Execution**: Sequential (Search determines path)
> **Critical Path**: Search Git → (Restore OR Rebuild) → Verify

---

## Context

### Original Request
The user states that "robust Swagger docs" previously existed at `/docs` but are now broken/gone. They need to be fixed.

### Investigation Findings
- No current `swagger` or `openapi` files in the codebase.
- No `/docs` route in `app.rb`.
- `Gemfile` does not contain `rswag` or `grape-swagger`.
- `controllers/api/v1/api_controller.rb` contains the active API implementation.

---

## Work Objectives

### Core Objective
Restore the `/docs` endpoint to serve functional Swagger UI documentation for the API.

### Concrete Deliverables
- [ ] `/docs` endpoint returns 200 OK and renders Swagger UI.
- [ ] Swagger UI successfully loads an OpenAPI spec.
- [ ] The spec accurately reflects the endpoints in `api_controller.rb`.

### Definition of Done
- [ ] `curl http://localhost:4567/docs` returns HTML with Swagger UI assets.
- [ ] Swagger UI page loads without console errors.
- [ ] API calls can be tested via the "Try it out" button in the UI.

### Must Have
- Functional "Try it out" feature.
- Accurate parameter definitions for key endpoints.

### Must NOT Have (Guardrails)
- Do NOT rewrite API logic.
- Do NOT add heavy dependencies if a static file works.
- Do NOT document internal/admin-only routes unless explicitly requested.

---

## Verification Strategy

> **UNIVERSAL RULE: ZERO HUMAN INTERVENTION**
> ALL tasks must be verifiable by the agent.

### Test Strategy
- **Infrastructure**: Existing Sinatra app.
- **Automated Tests**: No unit tests for docs; use Agent-Executed QA.
- **Agent-Executed QA**:
  - `curl` to verify route availability.
  - `playwright` (if available) or `grep` to verify HTML content.

---

## Execution Strategy

### Phase 1: Archaeology (Search & Restore)

> First, we attempt to find the deleted code. This is the preferred path.

- [ ] 1. **Search Git History**
  **What to do**:
  - Run `git log --all --full-history -- "**/swagger*"` to find deleted files.
  - Run `git log -S "swagger" --source --all` to find commits that added/removed "swagger".
  - Run `git log -S "/docs" --source --all` to find route definition.
  
  **Acceptance Criteria**:
  - [ ] Commit hash of deletion identified OR confirmed "not found".

- [ ] 2. **Conditional Restore**
  **What to do**:
  - IF found in Task 1:
    - `git checkout [hash]^ -- path/to/deleted/file`
    - Restore the `/docs` route in `app.rb` based on the old diff.
  - IF NOT found:
    - Mark this task as "Skipped - Proceed to Phase 2".

---

## Execution Strategy (Phase 2: Reconstruction)

> **ONLY execute if Phase 1 fails.**
> We will build a static Swagger UI setup.

- [ ] 3. **Create OpenAPI Spec Skeleton**
  **What to do**:
  - Create `public/openapi.yaml`.
  - Define basic info (Title: "Brilliant Little Gnome API", Version: "v1").
  - Define Server URL (`http://localhost:4567`).

- [ ] 4. **Document Endpoints (Reverse Engineer)**
  **What to do**:
  - Read `controllers/api/v1/api_controller.rb`.
  - Add paths to `openapi.yaml` for:
    - `GET /api/v1/courses`
    - `GET /api/v1/dashboard/summary`
    - `GET /api/v1/auth/cookies` (if public)
  - Define parameters and response schemas based on the code.

  **References**:
  - `controllers/api/v1/api_controller.rb`: Source of truth for endpoints.

- [ ] 5. **Implement Swagger UI View**
  **What to do**:
  - Create `views/docs.erb`.
  - Add HTML boilerplate importing Swagger UI via CDN (unpkg/cdnjs).
  - Configure it to load `/openapi.yaml`.
  - **Reference**: `https://github.com/swagger-api/swagger-ui/blob/master/dist/index.html` (Use this structure).

- [ ] 6. **Register Route**
  **What to do**:
  - Edit `app.rb`.
  - Add:
    ```ruby
    get '/docs' do
      erb :docs, layout: false
    end
    ```

---

## Success Criteria

### Verification Commands
```bash
# Verify route exists
curl -I http://localhost:4567/docs

# Verify spec exists
curl -I http://localhost:4567/openapi.yaml
```

### Final Checklist
- [ ] `/docs` is accessible.
- [ ] Swagger UI renders.
- [ ] Spec covers main V1 endpoints.
