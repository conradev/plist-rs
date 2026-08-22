/* Proof-only direct relational adapters. */

typedef __UINT8_TYPE__ uint8_t;
typedef __UINT64_TYPE__ uint64_t;

extern uint64_t saw_apple_get_sized_int_n(const uint8_t *, uint8_t);
extern uint64_t saw_port_sized_int_n(const uint8_t *, uint8_t);
extern uint64_t saw_production_sized_int_n(const uint8_t *, uint8_t);
extern uint8_t saw_apple_read_int_success_projection(
    const uint8_t *, uint8_t, uint8_t *);
extern uint8_t saw_port_read_int_success_projection(
    const uint8_t *, uint8_t, uint8_t *);
extern uint8_t saw_production_read_int_success_projection(
    const uint8_t *, uint8_t, uint8_t *);

uint8_t saw_c_port_equivalent_n(const uint8_t *data, uint8_t width) {
    return saw_apple_get_sized_int_n(data, width) ==
           saw_port_sized_int_n(data, width);
}

uint8_t saw_port_production_equivalent_n(const uint8_t *data, uint8_t width) {
    return saw_port_sized_int_n(data, width) ==
           saw_production_sized_int_n(data, width);
}

uint8_t saw_c_production_equivalent_n(const uint8_t *data, uint8_t width) {
    return saw_apple_get_sized_int_n(data, width) ==
           saw_production_sized_int_n(data, width);
}

static uint8_t saw_equal_16(const uint8_t *left, const uint8_t *right) {
    for (uint8_t index = 0; index < 16; ++index) {
        if (left[index] != right[index]) return 0;
    }
    return 1;
}

uint8_t saw_c_port_read_int_projection_equivalent(
    const uint8_t *data,
    uint8_t marker
) {
    uint8_t left[16];
    uint8_t right[16];
    saw_apple_read_int_success_projection(data, marker, left);
    saw_port_read_int_success_projection(data, marker, right);
    return saw_equal_16(left, right);
}

uint8_t saw_port_production_read_int_projection_equivalent(
    const uint8_t *data,
    uint8_t marker
) {
    uint8_t left[16];
    uint8_t right[16];
    saw_port_read_int_success_projection(data, marker, left);
    saw_production_read_int_success_projection(data, marker, right);
    return saw_equal_16(left, right);
}

uint8_t saw_c_production_read_int_projection_equivalent(
    const uint8_t *data,
    uint8_t marker
) {
    uint8_t left[16];
    uint8_t right[16];
    saw_apple_read_int_success_projection(data, marker, left);
    saw_production_read_int_success_projection(data, marker, right);
    return saw_equal_16(left, right);
}
