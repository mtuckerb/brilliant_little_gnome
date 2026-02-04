import json
import sqlite3
from datetime import datetime

# Load data
with open('psy220_definitions.json') as f:
    definitions = json.load(f)
with open('psy220_values.json') as f:
    values = json.load(f)

course_id = '446900'
db_path = 'db/development.sqlite3'

# Map values
values_map = {}
for v in values:
    obj_id = str(v.get('GradeObjectIdentifier') or v.get('Identifier'))
    values_map[obj_id] = v

# Map definitions
definitions_map = {}
for d in definitions:
    obj_id = str(d.get('Identifier') or d.get('Id'))
    definitions_map[obj_id] = d

all_ids = set(definitions_map.keys()) | set(values_map.keys())

conn = sqlite3.connect(db_path)
cursor = conn.cursor()

now = datetime.utcnow().iso8601 if hasattr(datetime, 'iso8601') else datetime.utcnow().strftime('%Y-%m-%d %H:%M:%S')

upserted_count = 0
for obj_id in all_ids:
    defn = definitions_map.get(obj_id, {})
    val = values_map.get(obj_id, {})
    
    name = defn.get('Name') or val.get('GradeObjectName') or val.get('Name') or f"Grade Item {obj_id}"
    
    # Simple denominator logic
    denominator = val.get('PointsDenominator') or val.get('GradeValue', {}).get('Denominator') or defn.get('MaxPoints')
    
    # Simple numerator logic
    numerator = val.get('PointsNumerator') or val.get('GradeValue', {}).get('Numerator')
    
    displayed_grade = val.get('DisplayedGrade')
    weight = defn.get('Weight') or val.get('Weight') or val.get('WeightedNumerator')
    is_extra_credit = 1 if defn.get('IsBonus') or defn.get('IsExtraCredit') else 0
    grade_object_type = defn.get('GradeType') or val.get('GradeObjectTypeName')

    # Upsert
    cursor.execute("""
        INSERT INTO grades (course_id, brightspace_id, name, displayed_grade, numerator, denominator, weight, is_extra_credit, grade_object_type, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(course_id, brightspace_id) DO UPDATE SET
            name=excluded.name,
            displayed_grade=excluded.displayed_grade,
            numerator=excluded.numerator,
            denominator=excluded.denominator,
            weight=excluded.weight,
            is_extra_credit=excluded.is_extra_credit,
            grade_object_type=excluded.grade_object_type,
            updated_at=excluded.updated_at
    """, (course_id, obj_id, name, displayed_grade, numerator, denominator, weight, is_extra_credit, grade_object_type, now, now))
    upserted_count += 1

conn.commit()
conn.close()

print(f"Successfully upserted {upserted_count} grades for course {course_id}")
