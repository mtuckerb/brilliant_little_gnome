class CreateDiscussionTables < ActiveRecord::Migration[5.2]
  def change
    create_table :discussion_forums do |t|
      t.string :brightspace_id
      t.string :course_id
      t.string :name
      t.text :description
      t.timestamps
    end
    add_index :discussion_forums, :brightspace_id
    add_index :discussion_forums, :course_id

    create_table :discussion_topics do |t|
      t.string :brightspace_id
      t.string :course_id
      t.string :forum_id # references forum brightspace_id
      t.string :name
      t.text :description
      t.integer :sort_order
      t.integer :thread_count, default: 0
      t.integer :post_count, default: 0
      t.datetime :last_post_date
      t.timestamps
    end
    add_index :discussion_topics, :brightspace_id
    add_index :discussion_topics, :course_id
    add_index :discussion_topics, :forum_id

    create_table :discussion_threads do |t|
      t.string :brightspace_id
      t.string :course_id
      t.string :topic_id # references topic brightspace_id
      t.string :subject
      t.text :body
      t.string :author_name
      t.datetime :posted_at
      t.boolean :is_pinned, default: false
      t.integer :unread_count, default: 0
      t.timestamps
    end
    add_index :discussion_threads, :brightspace_id
    add_index :discussion_threads, :course_id
    add_index :discussion_threads, :topic_id
  end
end
