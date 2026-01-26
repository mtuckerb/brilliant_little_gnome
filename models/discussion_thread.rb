class DiscussionThread < ActiveRecord::Base
  include HasUserIdentity
  validates :brightspace_id, presence: true, uniqueness: { scope: :topic_id }
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id
  belongs_to :discussion_topic, foreign_key: :topic_id, primary_key: :brightspace_id
end
