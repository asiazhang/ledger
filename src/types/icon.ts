import * as Icons from '@vicons/ionicons5'
import type { Component } from 'vue'

const iconRegistry = Icons as Record<string, Component>

export function getIconComponent(name: string | null): Component | null {
  if (!name) return null
  return iconRegistry[name] ?? null
}
