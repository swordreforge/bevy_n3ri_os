#ifndef V2_C_API_H
#define V2_C_API_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ───────── Opaque handle ───────── */
typedef struct V2Model V2Model;

/* ──────── Lifecycle ──────── */
V2Model* v2_model_create(void);
void     v2_model_destroy(V2Model* m);

/* ──────── Loading ──────── */
int v2_model_load_json(V2Model* m, const char* path);

/* ──────── Viewport ──────── */
void v2_model_resize(V2Model* m, int w, int h);
void v2_model_set_offset(V2Model* m, float dx, float dy);
void v2_model_set_scale(V2Model* m, float s);
void v2_model_rotate(V2Model* m, float deg);

/* ──────── Interaction ──────── */
void v2_model_drag(V2Model* m, float x, float y);
int  v2_model_is_motion_finished(V2Model* m);
const char* v2_model_get_current_group(V2Model* m);
int         v2_model_get_current_no(V2Model* m);

/* ──────── Parameters ──────── */
int     v2_model_get_param_count(V2Model* m);
float   v2_model_get_param_value(V2Model* m, int index);
float   v2_model_get_param_min(V2Model* m, int index);
float   v2_model_get_param_max(V2Model* m, int index);
float   v2_model_get_param_default(V2Model* m, int index);
const char* v2_model_get_param_id(V2Model* m, int index);
void    v2_model_set_param_value(V2Model* m, const char* id, float val, float weight);
void    v2_model_add_param_value(V2Model* m, const char* id, float val, float weight);

/* ──────── Parts ──────── */
int         v2_model_get_part_count(V2Model* m);
const char* v2_model_get_part_id(V2Model* m, int index);
void        v2_model_set_part_opacity(V2Model* m, int index, float val);

/* ──────── Motion / Expression ──────── */
void v2_model_start_motion(V2Model* m, const char* group, int no, int priority);
void v2_model_start_random_motion(V2Model* m, const char* group, int priority);
void v2_model_clear_motions(V2Model* m);
void v2_model_reset_pose(V2Model* m);
void v2_model_set_expression(V2Model* m, const char* name);
void v2_model_set_random_expression(V2Model* m);
void v2_model_reset_expression(V2Model* m);

/* ──────── Auto ──────── */
int  v2_model_get_auto_breath(V2Model* m);
void v2_model_set_auto_breath(V2Model* m, int v);
int  v2_model_get_auto_blink(V2Model* m);
void v2_model_set_auto_blink(V2Model* m, int v);

/* ──────── Update / Draw ──────── */
void v2_model_update(V2Model* m);
void v2_model_draw(V2Model* m);

/* ──────── Canvas info ──────── */
float v2_model_get_canvas_width(V2Model* m);
float v2_model_get_canvas_height(V2Model* m);
int   v2_model_get_pixels_per_unit(V2Model* m);

/* ──────── Hit test ──────── */
int  v2_model_hit_test(V2Model* m, const char* area, float x, float y);
/* Returns number of hit part IDs; out_ids and out_count are set on success.
   Caller must free with v2_free_string_array(). */
int  v2_model_hit_part(V2Model* m, float x, float y, int top_only,
                        const char*** out_ids, int* out_count);

/* ──────── Part color ──────── */
void v2_model_set_part_screen_color(V2Model* m, int index, float r, float g, float b, float a);
void v2_model_set_part_multiply_color(V2Model* m, int index, float r, float g, float b, float a);
void v2_model_get_part_screen_color(V2Model* m, int index, float out[4]);
void v2_model_get_part_multiply_color(V2Model* m, int index, float out[4]);

/* ──────── Texture ──────── */
void v2_model_set_texture(V2Model* m, int no, int tex_id);

/* ──────── String memory ──────── */
void v2_free_string(const char* s);
void v2_free_string_array(const char** ids, int count);

/* ──────── Module-level ──────── */
int  v2_gl_init(void);
void v2_clear_buffer(float r, float g, float b, float a);

#ifdef __cplusplus
}
#endif

#endif /* V2_C_API_H */
