class Grade < ActiveRecord::Base
  validates :brightspace_id, presence: true, uniqueness: { scope: :course_id }
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id

  def percentage
    return 0 if denominator.nil? || denominator == 0
    ((numerator.to_f / denominator.to_f) * 100).round(2)
  end

  def is_graded?
    # Brightspace often returns 0/X for ungraded items or items not yet submitted.
    # However, some items are legitimately 0 (missed assignments).
    # A safer check for "ungraded" in the context of a student view is often 
    # if the numerator is nil OR if the numerator is 0 and it's a 'Points' type
    # and the displayed grade is empty or '-'
    !numerator.nil? && !(numerator == 0 && (displayed_grade.nil? || displayed_grade.strip == "" || displayed_grade == "-"))
  end

  def self.calculate_weighted_total(course_id)
    grades = where(course_id: course_id.to_s)
    
    total_weight_possible = (grades.sum(:weight) || 0).to_f
    
    # Filter for items that have weights and are actually graded
    graded_items = grades.select { |g| (g.weight || 0) > 0 && g.is_graded? }
    
    if graded_items.empty?
      return { 
        score: 0, 
        confidence: 0, 
        total_weight_graded: 0, 
        total_weight_possible: total_weight_possible
      } 
    end
    
    if total_weight_possible <= 0
      return { 
        score: 0, 
        confidence: 0, 
        total_weight_graded: 0, 
        total_weight_possible: 0 
      } 
    end

    weighted_sum = 0
    total_weight_graded = 0

    graded_items.each do |g|
      weighted_sum += (g.percentage / 100.0) * g.weight
      total_weight_graded += g.weight
    end

    # The actual score is the sum of weighted percentages / total weight of items graded so far
    # Or, if we want the "current grade", we normalize it to 100% of the graded portion.
    current_score = (weighted_sum / total_weight_graded) * 100
    
    # Confidence is the percentage of the syllabus (by weight) that has been graded
    confidence = (total_weight_graded / total_weight_possible) * 100

    {
      score: current_score.round(2),
      confidence: confidence.round(1),
      total_weight_graded: total_weight_graded.round(2),
      total_weight_possible: total_weight_possible.round(2)
    }
  end
end
