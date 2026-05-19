class AddExpectedScoreToGrades < ActiveRecord::Migration[7.2]
  def change
    add_column :grades, :expected_score, :float
  end
end
