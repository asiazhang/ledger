export interface ExchangeRate {
  id: string
  base_code: string
  quote_code: string
  rate: number
  priced_at: string
  source: string | null
  updated_at: string
  version: number
  device_id: string
}

export interface ExchangeRateInput {
  base_code: string
  quote_code: string
  rate: number
  priced_at: string
  source?: string | null
}
