class ApiCache < ActiveRecord::Base
  include HasUserIdentity
  validates :url, presence: true

  scope :active, -> { where(is_archived: false) }
end
