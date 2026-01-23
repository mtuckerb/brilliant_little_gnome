class AddAttachmentsAndHtmlToTables < ActiveRecord::Migration[7.2]
  def change
    add_column :assignments, :attachments, :text
    add_column :content_items, :attachments, :text
    add_column :discussion_topics, :attachments, :text
    add_column :notifications, :attachments, :text
  end
end
