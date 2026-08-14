# frozen_string_literal: true

require_relative 'test_helper'

class TestField < Minitest::Test
  Field = Cosmos::Llm::Signature::Field

  def test_normalizes_class_types
    assert_equal :string, Field.new(:a, kind: :input, type: String).type
    assert_equal :integer, Field.new(:a, kind: :input, type: Integer).type
    assert_equal :number, Field.new(:a, kind: :input, type: Float).type
    assert_equal :array, Field.new(:a, kind: :input, type: Array).type
    assert_equal :object, Field.new(:a, kind: :input, type: Hash).type
  end

  def test_normalizes_symbol_aliases
    assert_equal :string, Field.new(:a, kind: :input, type: :str).type
    assert_equal :integer, Field.new(:a, kind: :input, type: :int).type
    assert_equal :number, Field.new(:a, kind: :input, type: :float).type
    assert_equal :boolean, Field.new(:a, kind: :input, type: :bool).type
  end

  def test_rejects_unknown_type
    error = assert_raises(Cosmos::Llm::Signature::DefinitionError) do
      Field.new(:a, kind: :input, type: :banana)
    end
    assert_match(/Unknown field type/, error.message)
  end

  def test_rejects_bad_kind
    assert_raises(Cosmos::Llm::Signature::DefinitionError) do
      Field.new(:a, kind: :sideways, type: String)
    end
  end

  def test_coerces_integers_from_strings
    field = Field.new(:count, kind: :output, type: Integer)

    assert_equal 42, field.coerce('42')
    assert_equal 42, field.coerce(' 42 ')
    assert_equal 42, field.coerce(42)
  end

  def test_coerces_numbers_from_strings
    field = Field.new(:score, kind: :output, type: Float)

    assert_in_delta 0.9, field.coerce('0.9')
    assert_in_delta 1.0, field.coerce(1)
  end

  def test_raises_on_uncoercible_number
    field = Field.new(:score, kind: :output, type: Float)
    error = assert_raises(Cosmos::Llm::Signature::CoercionError) { field.coerce('banana') }
    assert_match(/expected a number/, error.message)
  end

  def test_coerces_booleans
    field = Field.new(:flag, kind: :output, type: :boolean)

    assert_equal true, field.coerce('yes')
    assert_equal true, field.coerce('TRUE')
    assert_equal false, field.coerce('no')
    assert_equal false, field.coerce(false)
  end

  def test_raises_on_uncoercible_boolean
    field = Field.new(:flag, kind: :output, type: :boolean)
    assert_raises(Cosmos::Llm::Signature::CoercionError) { field.coerce('maybe') }
  end

  def test_coerces_array_from_json
    field = Field.new(:tags, kind: :output, type: Array)

    assert_equal %w[a b], field.coerce('["a", "b"]')
    assert_equal %w[a b], field.coerce(%w[a b])
  end

  def test_coerces_array_elements_to_declared_type
    field = Field.new(:counts, kind: :output, type: Array, of: Integer)

    assert_equal [1, 2, 3], field.coerce('["1", "2", 3]')
  end

  def test_coerces_object_from_json
    field = Field.new(:meta, kind: :output, type: Hash)

    assert_equal({ 'a' => 1 }, field.coerce('{"a": 1}'))
  end

  def test_raises_when_json_is_wrong_shape
    field = Field.new(:meta, kind: :output, type: Hash)
    assert_raises(Cosmos::Llm::Signature::CoercionError) { field.coerce('[1, 2]') }
  end

  def test_stringifies_structures_for_string_fields
    field = Field.new(:text, kind: :output, type: String)

    assert_equal '{"a":1}', field.coerce({ 'a' => 1 })
  end

  def test_enum_accepts_declared_values
    field = Field.new(:mood, kind: :output, type: String, enum: %w[good bad])

    assert_equal 'good', field.coerce('good')
  end

  def test_enum_rejects_other_values
    field = Field.new(:mood, kind: :output, type: String, enum: %w[good bad])
    assert_raises(Cosmos::Llm::Signature::CoercionError) { field.coerce('ugly') }
  end

  def test_nil_returns_default
    field = Field.new(:count, kind: :output, type: Integer, default: 7)

    assert_equal 7, field.coerce(nil)
  end

  def test_json_schema_for_scalar
    field = Field.new(:answer, kind: :output, type: String, desc: 'the answer')

    assert_equal({ 'type' => 'string', 'description' => 'the answer' }, field.to_json_schema)
  end

  def test_json_schema_for_typed_array
    field = Field.new(:tags, kind: :output, type: Array, of: String)

    assert_equal({ 'type' => 'array', 'items' => { 'type' => 'string' } }, field.to_json_schema)
  end

  def test_json_schema_includes_enum
    field = Field.new(:mood, kind: :output, type: String, enum: %w[good bad])

    assert_equal %w[good bad], field.to_json_schema['enum']
  end

  def test_type_hint_describes_arrays
    field = Field.new(:tags, kind: :output, type: Array, of: String)

    assert_equal 'a JSON array of strings', field.type_hint
  end

  def test_type_hint_includes_enum_values
    field = Field.new(:mood, kind: :output, type: String, enum: %w[good bad])

    assert_equal 'a string, one of: good, bad', field.type_hint
  end
end
