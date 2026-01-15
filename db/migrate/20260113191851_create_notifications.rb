class CreateNotifications < ActiveRecord::Migration[5.2]
  def change
    create_table :notifications do |t|
      t.string :external_id, index: true
      t.string :notification_type # News, Grade, Discussion
      t.string :title
      t.text :body
      t.datetime :date
      t.string :course_id, index: true
      t.string :course_name
      t.string :semester
      t.integer :urgency
      t.boolean :is_personal
      t.string :url
      t.timestamps
    end

    add_index :notifications, [:external_id, :course_id], unique: true
  end
end
