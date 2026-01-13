class Notification < ActiveRecord::Base
  validates :external_id, presence: true
end
