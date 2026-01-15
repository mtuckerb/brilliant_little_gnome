class AddWeightsToGrades < ActiveRecord::Migration[5.2]
  def change
    add_column :grades, :weight, :float
  end
end
