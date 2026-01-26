module HasUserIdentity
  extend ActiveSupport::Concern

  included do
    before_validation :ensure_user_identity
    
    # Optional: Scope all queries to the current user if brightspace_uid is present
    # This might be too aggressive for a first step, so we'll stick to assignment
  end

  private

  def ensure_user_identity
    self.user_id ||= UserPreference.current.brightspace_uid if self.respond_to?(:user_id=)
  end
end
