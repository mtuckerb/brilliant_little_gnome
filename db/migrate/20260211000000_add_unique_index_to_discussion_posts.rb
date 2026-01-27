class AddUniqueIndexToDiscussionPosts < ActiveRecord::Migration[7.2]
  def change
    # Remove existing non-unique index if it exists
    if index_exists?(:discussion_posts, :brightspace_id)
      remove_index :discussion_posts, :brightspace_id
    end
    
    # Add unique index
    add_index :discussion_posts, [:topic_id, :brightspace_id], unique: true, name: 'index_discussion_posts_on_topic_and_bs_id'
  end
end
