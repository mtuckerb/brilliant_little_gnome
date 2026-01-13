class Course < ActiveRecord::Base
  validates :org_unit_id, presence: true, uniqueness: true
  has_many :content_modules, foreign_key: :course_id, primary_key: :org_unit_id
  has_many :assignments, foreign_key: :course_id, primary_key: :org_unit_id
end
