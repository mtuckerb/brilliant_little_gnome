class ApiCache < ActiveRecord::Base
  validates :path, presence: true, uniqueness: true
end
