# Recipe: Polyglot Identity Implementation

Follow these steps to complete the transition to a multi-user, identity-aware architecture.

## Context
- The SOW is located at `/features/01_Polyglot_Identity`.
- Database migration is created at `db/migrate/20260126180000_enable_polyglot_identity.rb`.
- `UserPreference` and `BrilliantClient` have been updated to capture `brightspace_uid` and `brightspace_user_id`.

## Tasks
1. **Run Database Migrations:**
   - Execute `rake db:migrate` to add `user_id` to all relevant tables.
2. **Implement Identity Concern:**
   - Create `app/models/concerns/has_user_identity.rb`.
   - Implement `before_validation` hook to ensure `user_id` is populated.
   - Include this concern in all models that now have a `user_id` column.
3. **Data Claim Service:**
   - Create a service or rake task to populate `user_id` for existing orphaned records using the current `brightspace_uid`.
4. **JWT Authentication:**
   - Implement a JWT issuance service in `lib/brilliant/auth.rb`.
   - Update the API middleware (likely in `app.rb` or a dedicated middleware file) to validate JWTs.
5. **Discussion Sorting Fix:**
   - Update discussion controllers to use the integer `brightspace_user_id` for "Mine & Replied" filtering instead of name matching.
6. **Frontend Updates:**
   - Update the renderer to store the JWT and include it in the `Authorization` header for all requests.
