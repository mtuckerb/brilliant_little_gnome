class Notification < ActiveRecord::Base
  include HasUserIdentity
  validates :body, presence: true
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id
end
