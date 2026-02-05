import { createClient } from "@supabase/supabase-js";

const supabaseUrl = import.meta.env.VITE_SUPABASE_URL
const supabaseAnonKey = import.meta.env.VITE_SUPABASE_ANON_KEY

// Debug: log if variables are missing (only in dev)
if (!supabaseUrl || !supabaseAnonKey) {
  console.error('Supabase environment variables missing!')
  console.error('VITE_SUPABASE_URL:', supabaseUrl ? 'set' : 'MISSING')
  console.error('VITE_SUPABASE_ANON_KEY:', supabaseAnonKey ? 'set' : 'MISSING')
}

export const supabase = createClient(
  supabaseUrl || '',
  supabaseAnonKey || ''
)