# frozen_string_literal: true

require_relative 'test_helper'

class TestSignature < Minitest::Test
  class Summarize < Cosmos::Llm::Signature
    instructions 'Summarize the document.'
    input :document, String, desc: 'raw text'
    output :summary, String, desc: 'three sentences'
    output :confidence, Float
  end

  def test_declares_input_and_output_fields
    assert_equal %i[document], Summarize.input_fields.keys
    assert_equal %i[summary confidence], Summarize.output_fields.keys
  end

  def test_reads_instructions
    assert_equal 'Summarize the document.', Summarize.instructions
  end

  def test_generates_default_instructions
    sig = Cosmos::Llm::Signature.build do
      input :question
      output :answer
    end

    assert_equal 'Given the fields `question`, produce the fields `answer`.', sig.instructions
  end

  def test_rejects_duplicate_field_names
    assert_raises(Cosmos::Llm::Signature::DefinitionError) do
      Cosmos::Llm::Signature.build do
        input :question
        output :question
      end
    end
  end

  def test_build_requires_a_block
    assert_raises(Cosmos::Llm::Signature::DefinitionError) { Cosmos::Llm::Signature.build }
  end

  def test_parses_shorthand
    sig = Cosmos::Llm::Signature.parse('question -> answer')

    assert_equal %i[question], sig.input_fields.keys
    assert_equal %i[answer], sig.output_fields.keys
    assert_equal :string, sig.output_fields[:answer].type
  end

  def test_parses_shorthand_with_types
    sig = Cosmos::Llm::Signature.parse('document, style -> summary: string, score: number')

    assert_equal %i[document style], sig.input_fields.keys
    assert_equal :number, sig.output_fields[:score].type
  end

  def test_parses_shorthand_with_array_element_type
    sig = Cosmos::Llm::Signature.parse('text -> tags: array[string]')

    assert_equal :array, sig.output_fields[:tags].type
    assert_equal :string, sig.output_fields[:tags].element_type
  end

  def test_rejects_shorthand_without_arrow
    assert_raises(Cosmos::Llm::Signature::DefinitionError) do
      Cosmos::Llm::Signature.parse('question answer')
    end
  end

  def test_rejects_shorthand_with_two_arrows
    assert_raises(Cosmos::Llm::Signature::DefinitionError) do
      Cosmos::Llm::Signature.parse('a -> b -> c')
    end
  end

  def test_rejects_shorthand_with_no_outputs
    assert_raises(Cosmos::Llm::Signature::DefinitionError) do
      Cosmos::Llm::Signature.parse('question ->')
    end
  end

  def test_rejects_malformed_shorthand_field
    assert_raises(Cosmos::Llm::Signature::DefinitionError) do
      Cosmos::Llm::Signature.parse('question -> the answer!')
    end
  end

  def test_coerce_accepts_a_class
    assert_equal Summarize, Cosmos::Llm::Signature.coerce(Summarize)
  end

  def test_coerce_accepts_a_string
    sig = Cosmos::Llm::Signature.coerce('question -> answer')

    assert_equal %i[answer], sig.output_fields.keys
  end

  def test_coerce_rejects_other_values
    assert_raises(Cosmos::Llm::Signature::DefinitionError) { Cosmos::Llm::Signature.coerce(42) }
  end

  def test_output_json_schema
    schema = Summarize.output_json_schema

    assert_equal 'object', schema['type']
    assert_equal %w[summary confidence], schema['properties'].keys
    assert_equal %w[summary confidence], schema['required']
    assert_equal false, schema['additionalProperties']
  end

  def test_coerce_outputs_applies_types
    result = Summarize.coerce_outputs('summary' => 'text', 'confidence' => '0.9')

    assert_equal 'text', result[:summary]
    assert_in_delta 0.9, result[:confidence]
  end

  def test_coerce_outputs_accepts_symbol_keys
    result = Summarize.coerce_outputs(summary: 'text', confidence: 0.5)

    assert_in_delta 0.5, result[:confidence]
  end

  def test_coerce_outputs_raises_on_missing_required
    error = assert_raises(Cosmos::Llm::Signature::MissingFieldError) do
      Summarize.coerce_outputs('summary' => 'text')
    end
    assert_match(/confidence/, error.message)
  end

  def test_coerce_outputs_omits_optional_fields
    sig = Cosmos::Llm::Signature.build do
      input :question
      output :answer
      output :note, String, required: false
    end

    result = sig.coerce_outputs('answer' => 'yes')

    assert_equal({ answer: 'yes' }, result)
  end

  def test_coerce_inputs_applies_types
    result = Summarize.coerce_inputs('document' => 'text')

    assert_equal({ document: 'text' }, result)
  end

  def test_with_instructions_returns_a_new_class
    variant = Summarize.with_instructions('Be terse.')

    assert_equal 'Be terse.', variant.instructions
    assert_equal 'Summarize the document.', Summarize.instructions
    assert_equal Summarize.fields.keys, variant.fields.keys
  end

  def test_derive_adds_fields_without_touching_the_parent
    extended = Summarize.derive { output :reasoning, String }

    assert_includes extended.output_fields.keys, :reasoning
    refute_includes Summarize.output_fields.keys, :reasoning
  end

  def test_cache_signature_is_stable_for_equivalent_signatures
    a = Cosmos::Llm::Signature.parse('question -> answer')
    b = Cosmos::Llm::Signature.parse('question -> answer')

    assert_equal a.cache_signature, b.cache_signature
  end

  def test_cache_signature_changes_with_instructions
    a = Cosmos::Llm::Signature.parse('question -> answer')
    b = a.with_instructions('Be terse.')

    refute_equal a.cache_signature, b.cache_signature
  end

  def test_to_s_describes_the_signature
    assert_match(/document -> summary, confidence/, Summarize.to_s)
  end
end
