# Draft: Restore Swagger Docs

## Situation
- User reports `/docs` is broken.
- **Investigation**: No traces of Swagger/OpenAPI found in `Gemfile`, `app.rb`, or file system. It appears to have been removed or never committed.

## Proposed Solution
Since the docs are missing, we will re-implement them:

1. **Add Swagger UI**:
   - Create `views/swagger.erb` serving Swagger UI (via CDN for simplicity, or local assets).
   - Add `/docs` route to `app.rb` to render this view.

2. **Create OpenAPI Spec (`public/openapi.yaml`)**:
   - Reverse-engineer the spec from `controllers/api/v1/api_controller.rb`.
   - Define paths for:
     - `/api/v1/auth/cookies`
     - `/api/v1/courses`
     - `/api/v1/dashboard/summary`
     - etc.

## Open Questions
- Should we use a Ruby gem (like `rswag`) or just a static YAML file?
  - *Recommendation*: Static YAML + Swagger UI is simpler for Sinatra and lighter weight.
