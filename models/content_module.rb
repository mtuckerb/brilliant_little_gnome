class ContentModule < ActiveRecord::Base
  include HasUserIdentity
  validates :brightspace_id, presence: true
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id
  has_many :content_items, foreign_key: :module_id, primary_key: :brightspace_id
  belongs_to :parent, class_name: 'ContentModule', foreign_key: :parent_id, primary_key: :brightspace_id
  has_many :sub_modules, class_name: 'ContentModule', foreign_key: :parent_id, primary_key: :brightspace_id
end
