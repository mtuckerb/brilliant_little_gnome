class ContentItem < ActiveRecord::Base
  validates :brightspace_id, presence: true
  belongs_to :content_module, foreign_key: :module_id, primary_key: :brightspace_id
end
