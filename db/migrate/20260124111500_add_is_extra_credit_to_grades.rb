class AddIsExtraCreditToGrades < ActiveRecord::Migration[7.2]
  def change
    add_column :grades, :is_extra_credit, :boolean, default: false
  end
end
