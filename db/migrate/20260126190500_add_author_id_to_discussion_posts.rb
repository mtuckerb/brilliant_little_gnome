class AddAuthorIdToDiscussionPosts < ActiveRecord::Migration[7.2]
  def change
    unless column_exists?(:discussion_posts, :author_id)
      add_column :discussion_posts, :author_id, :integer
      add_index :discussion_posts, :author_id
    end
  end
end
