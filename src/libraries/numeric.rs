use crate::{
    Diagnostic, Location, RuntimeHost,
    heap::{Heap, Value},
};

use super::{invalid_arguments, pointer};

pub(super) const RAND: &str = "
    struct rand;
    typedef struct rand* rand_t;
    rand_t init_rand(int seed);
    int rand(rand_t generator);
";

pub(super) const UTIL: &str = "
    #use <string>
    int int_size();
    int int_max();
    int int_min();
    int abs(int x) /*@requires x > int_min(); @*/ ;
    int max(int x, int y);
    int min(int x, int y);
    string int2hex(int x);
";

pub(super) const FPT: &str = "
    typedef int fpt;
    fpt fadd(fpt left, fpt right);
    fpt fsub(fpt left, fpt right);
    fpt fmul(fpt left, fpt right);
    fpt fdiv(fpt left, fpt right);
    bool fless(fpt left, fpt right);
    fpt itof(int number);
    int ftoi(fpt number);
    void print_fpt(fpt number);
    void print_int(int number);
    void print_hex(int number);
";

pub(super) const DUB: &str = "
    struct dub;
    typedef struct dub* dub;
    dub dadd(dub left, dub right);
    dub dsub(dub left, dub right);
    dub dmul(dub left, dub right);
    dub ddiv(dub left, dub right);
    bool dless(dub left, dub right);
    dub itod(int number);
    int dtoi(dub number);
    void print_dub(dub number);
";

fn float(value: i32) -> f32 {
    f32::from_bits(value.cast_unsigned())
}

fn float_value(value: f32) -> Value {
    Value::Int(value.to_bits().cast_signed())
}

fn double(value: &Value, location: Location) -> Result<f64, Diagnostic> {
    let Value::Pointer(Some(memory)) = value else {
        return Err(location.error("double-precision argument is NULL"));
    };
    match memory.borrow().first() {
        Some(Value::Float(value)) => Ok(*value),
        _ => Err(location.error("invalid double-precision handle")),
    }
}

fn truncate(value: f64) -> i32 {
    let value = value.trunc();
    if value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX) {
        value as i32
    } else {
        i32::MIN
    }
}

pub(super) fn call(
    name: &str,
    args: &[Value],
    host: &mut impl RuntimeHost,
    heap: &mut Heap,
    location: Location,
) -> Result<Value, Diagnostic> {
    let output = match (name, args) {
        ("int_size", []) => return Ok(Value::Int(4)),
        ("int_max", []) => return Ok(Value::Int(i32::MAX)),
        ("int_min", []) => return Ok(Value::Int(i32::MIN)),
        ("abs", [Value::Int(value)]) => return Ok(Value::Int(value.wrapping_abs())),
        ("max", [Value::Int(left), Value::Int(right)]) => return Ok(Value::Int(*left.max(right))),
        ("min", [Value::Int(left), Value::Int(right)]) => return Ok(Value::Int(*left.min(right))),
        ("int2hex", [Value::Int(value)]) => {
            return Ok(Value::string(format!("{value:08X}").into_bytes()));
        }
        ("init_rand", [Value::Int(seed)]) => return Ok(pointer(heap, Some(Value::Int(*seed)))),
        ("rand", [Value::Pointer(generator)]) => {
            let generator = generator
                .as_ref()
                .ok_or_else(|| location.error("random generator is NULL"))?;
            let mut state = generator.borrow_mut();
            let Some(Value::Int(seed)) = state.first_mut() else {
                return Err(location.error("invalid random generator handle"));
            };
            let value = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *seed = value;
            return Ok(Value::Int(value));
        }
        (
            op @ ("fadd" | "fsub" | "fmul" | "fdiv" | "fless"),
            [Value::Int(left), Value::Int(right)],
        ) => {
            let (left, right) = (float(*left), float(*right));
            return Ok(match op {
                "fadd" => float_value(left + right),
                "fsub" => float_value(left - right),
                "fmul" => float_value(left * right),
                "fdiv" => float_value(left / right),
                _ => Value::Bool(left < right),
            });
        }
        ("itof", [Value::Int(value)]) => return Ok(float_value(*value as f32)),
        ("ftoi", [Value::Int(value)]) => return Ok(Value::Int(truncate(f64::from(float(*value))))),
        ("print_fpt", [Value::Int(value)]) => format!("{:.6}", float(*value)),
        ("print_int", [Value::Int(value)]) => format!("{value}"),
        ("print_hex", [Value::Int(value)]) => format!("{value:x}"),
        (op @ ("dadd" | "dsub" | "dmul" | "ddiv" | "dless"), [left, right]) => {
            let (left, right) = (double(left, location)?, double(right, location)?);
            let value = match op {
                "dadd" => left + right,
                "dsub" => left - right,
                "dmul" => left * right,
                "ddiv" => left / right,
                _ => return Ok(Value::Bool(left < right)),
            };
            return Ok(pointer(heap, Some(Value::Float(value))));
        }
        ("itod", [Value::Int(value)]) => {
            return Ok(pointer(heap, Some(Value::Float(f64::from(*value)))));
        }
        ("dtoi", [value]) => return Ok(Value::Int(truncate(double(value, location)?))),
        ("print_dub", [value]) => format!("{:.6}", double(value, location)?),
        _ => return Err(invalid_arguments(name, location)),
    };
    host.write(output.as_bytes())
        .map_err(|error| location.error(format!("{name}: {error}")))?;
    Ok(Value::Void)
}
