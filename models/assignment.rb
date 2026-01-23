class Assignment < ActiveRecord::Base
  validates :brightspace_id, presence: true
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id

  def parsed_attachments
    JSON.parse(attachments || '[]')
  end
end
