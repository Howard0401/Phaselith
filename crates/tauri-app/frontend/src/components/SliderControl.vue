<template>
  <div class="slider-control">
    <div class="slider-header">
      <label>{{ label }}</label>
      <span class="value">{{ Math.round(modelValue * 100) }}%</span>
    </div>
    <input
      type="range"
      min="0"
      max="100"
      :value="Math.round(modelValue * 100)"
      @input="$emit('update:modelValue', $event.target.value / 100)"
    >
  </div>
</template>

<script setup>
defineProps({
  label: String,
  modelValue: Number,
});
defineEmits(['update:modelValue']);
</script>

<style scoped>
.slider-control {
  margin-bottom: 12px;
}
.slider-header {
  display: flex;
  justify-content: space-between;
  font-size: 13px;
  margin-bottom: 4px;
}
.value { color: #00d4ff; font-size: 12px; }
input[type="range"] {
  width: 100%;
  accent-color: #00d4ff;
  height: 4px;
}
</style>
