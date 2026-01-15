class CreateGradesTable < ActiveRecord::Migration[5.2]
  def change
    create_table :grades do |t|
      t.string :course_id
      t.string :brightspace_id
      t.string :name
      t.string :displayed_grade
      t.float :numerator
      t.float :denominator
      t.string :grade_object_type
      t.datetime :last_modified
      t.text :comments
      t.timestamps
    end
    add_index :grades, :course_id
    add_index :grades, :brightspace_id
    add_index :grades, [:course_id, :brightspace_id], unique: true
  end
end
