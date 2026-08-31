#include "v2_c_api.h"

#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "../Live2D/V2/Framework/LAppModel.hpp"
#include "../Live2D/V2/Graphics/DrawParamOpenGL.hpp"

extern "C" int gladLoadGL();

/* ───────── Internal helper: opaque handle ───────── */
struct V2Model {
    live2d::LAppModel* model;
    std::string last_str;                   // buffer for single-string returns
    std::vector<const char*> last_str_ptrs; // buffer for string-array returns
    std::vector<std::string> last_str_vec;  // owned storage for string array

    V2Model() : model(new live2d::LAppModel()) {}
    ~V2Model() { delete model; }

    // returns a strdup'd copy; caller must v2_free_string
    const char* dup(const std::string& s) {
        return ::strdup(s.c_str());
    }

    // Returns a pointer into a heap buffer that stays valid until the
    // next call to any v2_model_* function on this handle.
    // The caller does NOT free pointer returned by buf().
    // WARNING: reentrancy / multithreading not safe.
    const char* buf(const std::string& s) {
        last_str = s;
        return last_str.c_str();
    }

    // converts vector<string> -> vector<const char*>
    // pointers are into last_str_vec and are NOT owned by caller.
    void hold(const std::vector<std::string>& src) {
        last_str_vec = src;
        last_str_ptrs.clear();
        last_str_ptrs.reserve(src.size());
        for (auto& s : last_str_vec)
            last_str_ptrs.push_back(s.c_str());
    }
};

/* ──────── Lifecycle ──────── */
V2Model* v2_model_create(void) {
    try { return new V2Model(); }
    catch (...) { return nullptr; }
}

void v2_model_destroy(V2Model* m) {
    delete m;
}

/* ──────── Loading ──────── */
int v2_model_load_json(V2Model* m, const char* path) {
    try { return m->model->loadModelJson(path) ? 1 : 0; }
    catch (...) { return 0; }
}

/* ──────── Viewport ──────── */
void v2_model_resize(V2Model* m, int w, int h)           { m->model->resize(w, h); }
void v2_model_set_offset(V2Model* m, float dx, float dy) { m->model->setOffset(dx, dy); }
void v2_model_set_scale(V2Model* m, float s)             { m->model->setScale(s); }
void v2_model_rotate(V2Model* m, float deg)              { m->model->rotate(deg); }

/* ──────── Interaction ──────── */
void v2_model_drag(V2Model* m, float x, float y)         { m->model->drag(x, y); }
int  v2_model_is_motion_finished(V2Model* m)             { return m->model->isMotionFinished() ? 1 : 0; }

/* ──────── Parameters ──────── */
int   v2_model_get_param_count(V2Model* m)               { return m->model->getParameterCount(); }
float v2_model_get_param_value(V2Model* m, int index)    { return m->model->getParameterValue(index); }
float v2_model_get_param_min(V2Model* m, int index)      { return m->model->getParameterMin(index); }
float v2_model_get_param_max(V2Model* m, int index)      { return m->model->getParameterMax(index); }
float v2_model_get_param_default(V2Model* m, int index)  { return m->model->getParameterDefault(index); }

const char* v2_model_get_param_id(V2Model* m, int index) {
    return m->buf(m->model->getParameterId(index));
}

void v2_model_set_param_value(V2Model* m, const char* id, float val, float weight) {
    m->model->setParameterValue(id, val, weight);
}
void v2_model_add_param_value(V2Model* m, const char* id, float val, float weight) {
    m->model->addParameterValue(id, val, weight);
}

/* ──────── Parts ──────── */
int         v2_model_get_part_count(V2Model* m)              { return m->model->getPartCount(); }
const char* v2_model_get_part_id(V2Model* m, int index)      { return m->buf(m->model->getPartId(index)); }
void        v2_model_set_part_opacity(V2Model* m, int index, float val) { m->model->setPartOpacity(index, val); }

const char* v2_model_get_current_group(V2Model* m) {
    return m->buf(m->model->getCurrentGroup());
}
int v2_model_get_current_no(V2Model* m) {
    return m->model->getCurrentNo();
}

/* ──────── Motion / Expression ──────── */
void v2_model_start_motion(V2Model* m, const char* group, int no, int priority) {
    m->model->startMotion(group, no, priority);
}
void v2_model_start_random_motion(V2Model* m, const char* group, int priority) {
    m->model->startRandomMotion(group ? group : "", priority);
}
void v2_model_clear_motions(V2Model* m)          { m->model->clearMotions(); }
void v2_model_reset_pose(V2Model* m)             { m->model->resetPose(); }
void v2_model_set_expression(V2Model* m, const char* name) { m->model->setExpression(name); }
void v2_model_set_random_expression(V2Model* m)  { m->model->setRandomExpression(); }
void v2_model_reset_expression(V2Model* m)       { m->model->resetExpression(); }

/* ──────── Auto ──────── */
int  v2_model_get_auto_breath(V2Model* m)       { return m->model->mAutoBreath ? 1 : 0; }
void v2_model_set_auto_breath(V2Model* m, int v) { m->model->setAutoBreathEnable(v != 0); }
int  v2_model_get_auto_blink(V2Model* m)        { return m->model->mAutoBlink ? 1 : 0; }
void v2_model_set_auto_blink(V2Model* m, int v)  { m->model->setAutoBlinkEnable(v != 0); }

/* ──────── Update / Draw ──────── */
void v2_model_update(V2Model* m)  { m->model->update(); }
void v2_model_draw(V2Model* m)    { m->model->draw(); }

/* ──────── Canvas info ──────── */
float v2_model_get_canvas_width(V2Model* m)   { return m->model->getCanvasWidth(); }
float v2_model_get_canvas_height(V2Model* m)  { return m->model->getCanvasHeight(); }
int   v2_model_get_pixels_per_unit(V2Model* m) { return m->model->getPixelsPerUnit(); }

/* ──────── Hit test ──────── */
int v2_model_hit_test(V2Model* m, const char* area, float x, float y) {
    return m->model->hitTest(area, x, y) ? 1 : 0;
}

int v2_model_hit_part(V2Model* m, float x, float y, int top_only,
                       const char*** out_ids, int* out_count) {
    try {
        auto ids = m->model->hitPart(x, y, top_only != 0);
        if (ids.empty()) { *out_ids = nullptr; *out_count = 0; return 0; }
        m->hold(ids);
        *out_count = (int)m->last_str_ptrs.size();
        *out_ids   = m->last_str_ptrs.data();
        return *out_count;
    } catch (...) {
        *out_ids = nullptr; *out_count = 0; return -1;
    }
}

/* ──────── Part color ──────── */
void v2_model_set_part_screen_color(V2Model* m, int idx, float r, float g, float b, float a) {
    m->model->setPartScreenColor(idx, r, g, b, a);
}
void v2_model_set_part_multiply_color(V2Model* m, int idx, float r, float g, float b, float a) {
    m->model->setPartMultiplyColor(idx, r, g, b, a);
}
void v2_model_get_part_screen_color(V2Model* m, int idx, float out[4]) {
    auto c = m->model->getPartScreenColor(idx);
    if (c.size() >= 4) { out[0]=c[0]; out[1]=c[1]; out[2]=c[2]; out[3]=c[3]; }
}
void v2_model_get_part_multiply_color(V2Model* m, int idx, float out[4]) {
    auto c = m->model->getPartMultiplyColor(idx);
    if (c.size() >= 4) { out[0]=c[0]; out[1]=c[1]; out[2]=c[2]; out[3]=c[3]; }
}

/* ──────── Texture ──────── */
void v2_model_set_texture(V2Model* m, int no, int tex_id) {
    m->model->setTexture(no, tex_id);
}

/* ──────── String memory ──────── */
void v2_free_string(const char* s) {
    if (s) ::free(const_cast<char*>(s));
}
void v2_free_string_array(const char** ids, int count) {
    (void)ids; (void)count;
    // no-op: string array pointers are into V2Model::last_str_vec,
    // freed when the V2Model is destroyed.
}

/* ──────── Module-level ──────── */
int v2_gl_init(void) {
    return gladLoadGL();
}

void v2_clear_buffer(float r, float g, float b, float a) {
    live2d::DrawParamOpenGL::clearBuffer(r, g, b, a);
}
