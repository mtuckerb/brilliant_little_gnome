class Notification < ActiveRecord::Base
  include HasUserIdentity
  validates :message, presence: true
end
