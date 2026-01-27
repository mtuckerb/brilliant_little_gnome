class Notification < ActiveRecord::Base
  include HasUserIdentity
  validates :body, presence: true
end
