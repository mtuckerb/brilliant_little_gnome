class CreateDiscussionPosts < ActiveRecord::Migration[5.2]
  def change
    create_table :discussion_posts do |t|
      t.string :brightspace_id, index: true
      t.string :topic_id, index: true
      t.string :thread_id, index: true
      t.string :parent_post_id, index: true
      t.string :subject
      t.text :body
      t.string :author_name
      t.datetime :posted_at
      t.boolean :is_instructor, default: false
      t.timestamps
    end

    # Ensure api_caches exists if it was accidentally dropped
    unless table_exists?(:api_caches)
      create_table :api_caches do |t|
        t.string :path, index: true, unique: true
        t.text :data
        t.timestamps
      end
    end
  end
end
