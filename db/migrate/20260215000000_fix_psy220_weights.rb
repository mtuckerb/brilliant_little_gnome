class FixPsy220Weights < ActiveRecord::Migration[7.2]
  def up
    # PSY 220 (446900)
    # Based on observation, these should have weight 10 each
    Grade.where(course_id: '446900').where("name LIKE 'Chapter 1%'").update_all(weight: 10.0, is_extra_credit: false)
    Grade.where(course_id: '446900').where("name LIKE 'Chapter 2%'").update_all(weight: 10.0, is_extra_credit: false)
  end

  def down
  end
end
