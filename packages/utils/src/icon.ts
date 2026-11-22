import type { Component } from "vue";
import {
  BriefcaseOutline,
  BusOutline,
  CardOutline,
  CarOutline,
  CartOutline,
  EllipsisHorizontalOutline,
  FlashOutline,
  GiftOutline,
  GameControllerOutline,
  HomeOutline,
  MedkitOutline,
  PhonePortraitOutline,
  RestaurantOutline,
  SchoolOutline,
  TrendingUpOutline,
  TrophyOutline,
  WalletOutline,
} from "@vicons/ionicons5";

const iconRegistry: Record<string, Component> = {
  BriefcaseOutline,
  BusOutline,
  CardOutline,
  CarOutline,
  CartOutline,
  EllipsisHorizontalOutline,
  FlashOutline,
  GiftOutline,
  GameControllerOutline,
  HomeOutline,
  MedkitOutline,
  PhonePortraitOutline,
  RestaurantOutline,
  SchoolOutline,
  TrendingUpOutline,
  TrophyOutline,
  WalletOutline,
};

export function getIconComponent(name: string | null): Component | null {
  if (!name) return null;
  return iconRegistry[name] ?? null;
}
