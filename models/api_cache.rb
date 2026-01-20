class ApiCache < ActiveRecord::Base
  validates :path, presence: true, uniqueness: true

  scope :active, -> { where(is_archived: false) }
end
